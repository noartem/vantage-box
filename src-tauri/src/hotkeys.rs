//! Global hotkeys. Bindings live in `settings.json` and are re-registered on
//! every settings change.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::actions;
use crate::settings::Settings;
use crate::state::AppState;
use crate::window;

/// Event for the UI: the list of hotkey registration problems.
pub const EVENT_HOTKEYS: &str = "hotkeys://problems";

/// What a hotkey does.
#[derive(Clone, Copy)]
enum Action {
    /// Start or stop sing-box.
    Toggle,
    /// Show the proxy-selection popup.
    ProxyPopup,
    /// Show and focus the main window.
    ShowMain,
    /// Soft restart of the current run.
    Restart,
}

/// Re-registers all hotkeys for the current settings.
///
/// Returns a list of problems: a combination taken by another program, or a
/// typo in the settings, must be shown to the user and not silently swallowed
/// — otherwise the hotkey just "does not work" with no explanation.
pub fn apply(app: &AppHandle, settings: &Settings) -> Vec<String> {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();

    let mut problems = Vec::new();
    // Only the global actions are OS-registered here. The in-app shortcuts
    // (go-to-settings, tab cycling, tab-by-index, close window) live in the same
    // settings block but are matched against keydown events in the frontend.
    let bindings = [
        (settings.hotkeys.toggle.trim(), Action::Toggle, "toggle on/off"),
        (
            settings.hotkeys.proxy_popup.trim(),
            Action::ProxyPopup,
            "proxy selection popup",
        ),
        (
            settings.hotkeys.show_main.trim(),
            Action::ShowMain,
            "show main window",
        ),
        (
            settings.hotkeys.restart.trim(),
            Action::Restart,
            "restart current run",
        ),
    ];

    for (binding, action, description) in bindings {
        // An empty string is a deliberate opt-out of the hotkey, not an error.
        if binding.is_empty() {
            continue;
        }

        let result = shortcuts.on_shortcut(binding, move |app, _shortcut, event| {
            // Without this check the action would fire twice: on press and on
            // release.
            if event.state != ShortcutState::Pressed {
                return;
            }
            dispatch(app.clone(), action);
        });

        if let Err(e) = result {
            problems.push(format!("{description} ({binding}): {e}"));
        }
    }

    remember(app, &problems);
    problems
}

fn dispatch(app: AppHandle, action: Action) {
    match action {
        Action::ProxyPopup => window::toggle_popup(&app),
        Action::ShowMain => window::show_main(&app),
        Action::Toggle => {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = actions::toggle(&app).await {
                    eprintln!("toggle hotkey failed: {e}");
                }
                crate::tray::refresh(&app).await;
            });
        }
        Action::Restart => {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = actions::restart(&app).await {
                    eprintln!("restart hotkey failed: {e}");
                }
                crate::tray::refresh(&app).await;
            });
        }
    }
}

fn remember(app: &AppHandle, problems: &[String]) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.hotkey_problems.lock().expect("hotkey problems lock") = problems.to_vec();
    }
    let _ = app.emit(EVENT_HOTKEYS, problems.to_vec());
}