// Public modules: the integration test in `tests/` relies on them.
pub mod binary;
pub mod clash;
pub mod compat;
pub mod error;
pub mod fallback;
pub mod jsonc;
pub mod process;
pub mod runtime;
pub mod service;
pub mod settings;
pub mod subscription;

mod actions;
mod commands;
mod hotkeys;
mod selftest;
mod state;
mod tray;
mod window;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};

use clash::{ClashClient, StreamManager};
use settings::{Settings, SettingsStore};
use state::{AppState, EVENT_CONFIG_CHANGED, EVENT_SETTINGS_ERROR};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // SCM may have started this same binary as a service (`--scm`). In this
    // mode Tauri is not needed: we only report state to SCM and keep sing-box
    // as a child process. We check this branch before `tauri::Builder` —
    // otherwise under LocalSystem the window would not appear anyway, and the
    // wrapper would not have time to answer SCM.
    #[cfg(windows)]
    if service::scm::is_invocation() {
        // Connect to SCM and block until the service stops. On a separate
        // thread SCM will run service_main — from there we register the
        // handler and bring up sing-box. Tauri is not needed in this mode.
        service::scm::dispatch();
        return;
    }

    let mut builder = tauri::Builder::default();

    // The single-instance plugin must go first: it decides whether a second
    // process lives or hands control to the already running one.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app);
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // A broken settings.json must not block startup: we come up on
            // defaults and show the error in the UI so it can be fixed.
            let (initial, load_error) = match settings::load_or_create() {
                Ok(s) => (s, None),
                Err(e) => (Settings::default(), Some(e.to_string())),
            };

            let path = settings::settings_path()?;
            let store = state::share(SettingsStore::new(path.clone(), initial.clone()));

            let streams = Arc::new(StreamManager::new(handle.clone()));
            let client = ClashClient::new(&runtime::effective_api_settings(&initial))?;
            streams.restart(client.clone(), initial.clash_api.log_level);

            let (config_tx, mut config_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
            let app_state =
                AppState::new(Arc::clone(&store), client, Arc::clone(&streams), config_tx);
            app_state.rearm_config_watcher(&initial.sing_box.config_path);

            // Watch the file: manual edits in an editor must be picked up
            // without restarting the app.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
            match settings::spawn_watcher(&path, tx) {
                Ok(watcher) => {
                    *app_state.watcher.lock().expect("watcher lock") = Some(watcher);
                }
                Err(e) => {
                    eprintln!("settings watcher not started: {e}");
                }
            }

            app.manage(app_state);

            let hotkey_problems = hotkeys::apply(&handle, &initial);
            state::sync_autostart(&handle, initial.autostart);

            let tray_ready = if initial.tray.enabled {
                match tray::setup(&handle) {
                    Ok(()) => {
                        tray::spawn_refresher(handle.clone());
                        true
                    }
                    Err(e) => {
                        // Without the tray the app still works, so do not crash.
                        eprintln!("failed to create tray icon: {e}");
                        false
                    }
                }
            } else {
                false
            };

            let window_shown = setup_main_window(&handle, &initial);
            report_startup(&initial, tray_ready, &hotkey_problems, window_shown, &load_error);

            let watch_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut previous = initial;
                while rx.recv().await.is_some() {
                    let state = watch_handle.state::<AppState>();
                    match state.settings.reload() {
                        // The file changed but the contents are the same — for
                        // example, it was our own write.
                        Ok(None) => {}
                        Ok(Some(next)) => {
                            if let Err(e) =
                                state::apply_settings(&watch_handle, &state, &previous, &next)
                            {
                                let _ = watch_handle.emit(EVENT_SETTINGS_ERROR, e.to_string());
                            }
                            previous = next;
                        }
                        Err(e) => {
                            let _ = watch_handle.emit(EVENT_SETTINGS_ERROR, e.to_string());
                        }
                    }
                }
            });

            // External edits to config.json: tell the UI so it can offer to
            // re-read the file and softly restart sing-box.
            let config_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                while config_rx.recv().await.is_some() {
                    let state = config_handle.state::<AppState>();
                    let path = state.settings.get().sing_box.config_path;
                    if path.trim().is_empty() {
                        continue;
                    }
                    if state.config_changed_externally(&path) {
                        let _ = config_handle.emit(EVENT_CONFIG_CHANGED, path);
                    }
                }
            });

            if let Some(message) = load_error {
                let _ = handle.emit(EVENT_SETTINGS_ERROR, message);
            }

            // Pull the sing-box release catalog in the background: in the UI
            // it always comes from cache, and opening the tab must not wait on GitHub.
            if binary::catalog_is_stale() {
                tauri::async_runtime::spawn(async {
                    if let Err(e) = binary::refresh_catalog(None).await {
                        eprintln!("sing-box release catalog not updated: {e}");
                    }
                });
            }

            // Subscriptions: inject nodes at startup and refresh periodically.
            subscription::spawn_refresher(handle.clone());

            // Fallback monitor: auto-switch selector groups to a backup.
            fallback::spawn(handle.clone());

            if selftest::requested() {
                selftest::spawn(handle.clone());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::get_settings_path,
            commands::save_settings,
            commands::get_status,
            commands::get_proxies,
            commands::select_proxy,
            commands::test_group_delay,
            commands::test_proxy_delay,
            commands::get_connections,
            commands::close_connection,
            commands::close_all_connections,
            commands::refresh_subscriptions,
            commands::get_subscription_state,
            commands::read_singbox_config,
            commands::check_singbox_config,
            commands::write_singbox_config,
            commands::create_minimal_config,
            commands::get_run_status,
            commands::install_service,
            commands::uninstall_service,
            commands::start_service,
            commands::stop_service,
            commands::restart_service,
            commands::get_binary_info,
            commands::list_singbox_releases,
            commands::download_singbox_release,
            commands::delete_singbox_release,
            commands::use_singbox_release,
            commands::get_hotkey_problems,
            commands::close_popup,
            commands::show_main_window,
            commands::generate_secret,
            commands::pick_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// One line on what came up at startup and what did not.
///
/// The tray icon and global hotkeys are not visible from app logs and cannot
/// be checked from the outside: if a combination is taken by another program,
/// the only symptom is "the hotkey does not work". This line makes such
/// failures visible and lets startup be checked automatically
/// (see `scripts/smoke-test.ps1`).
fn report_startup(
    settings: &Settings,
    tray_ready: bool,
    hotkey_problems: &[String],
    window_shown: bool,
    load_error: &Option<String>,
) {
    let tray = match (settings.tray.enabled, tray_ready) {
        (false, _) => "off",
        (true, true) => "ok",
        (true, false) => "failed",
    };
    let hotkeys = if hotkey_problems.is_empty() {
        "ok".to_string()
    } else {
        format!("problems={}", hotkey_problems.len())
    };

    eprintln!(
        "vantage-box startup tray={tray} hotkeys={hotkeys} window={} settings={}",
        if window_shown { "shown" } else { "hidden" },
        if load_error.is_some() { "failed" } else { "ok" }
    );

    for problem in hotkey_problems {
        eprintln!("vantage-box hotkey problem: {problem}");
    }
}

/// The main window is created hidden (see `tauri.conf.json`), so that when
/// starting minimized it does not flash on screen.
///
/// Returns `true` if the window was shown.
fn setup_main_window(handle: &tauri::AppHandle, settings: &Settings) -> bool {
    let Some(main) = handle.get_webview_window(window::MAIN) else {
        return false;
    };

    // Hiding at startup only makes sense if the tray exists: otherwise the
    // app would be running with no way to open it.
    let start_hidden = settings.tray.enabled && settings.tray.start_minimized;
    if !start_hidden {
        let _ = main.show();
        let _ = main.set_focus();
    }

    let close_handle = handle.clone();
    main.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if window::is_quitting() {
                return;
            }
            let settings = close_handle.state::<AppState>().settings.get();
            if settings.tray.enabled && settings.tray.close_to_tray {
                api.prevent_close();
                window::hide_main(&close_handle);
            }
        }
    });

    !start_hidden
}
