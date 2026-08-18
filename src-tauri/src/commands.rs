//! Commands available to the frontend via `invoke`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::actions::{self, blocking, RestartOutcome, RunStatus};
use crate::binary::{self, BinaryInfo, CheckResult, ReleaseCatalog};
use crate::clash::models::{ConnectionStatus, ConnectionsSnapshot, Proxy};
use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;
use crate::process;
use crate::runtime;
use crate::service;
use crate::settings::{config_dir, Settings};
use crate::state::{self, AppState};
use crate::subscription::{self, ApplyOutcome, SubscriptionsState};

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.get()
}

#[tauri::command]
pub fn get_settings_path(state: State<'_, AppState>) -> String {
    state.settings.path().display().to_string()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings> {
    let previous = state.settings.get();
    state.settings.save(settings.clone())?;
    state::apply_settings(&app, &state, &previous, &settings)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> ConnectionStatus {
    state.streams.status()
}

// ---------------------------------------------------------------------------
// Proxies
// ---------------------------------------------------------------------------

/// The flat `/proxies` response is inconvenient for the UI, so we break it down
/// into groups with nodes already filled in and the latest latency measurements.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyOverview {
    pub groups: Vec<GroupView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub name: String,
    /// `Selector`, `URLTest`, …
    pub kind: String,
    pub now: Option<String>,
    /// Whether the selection can be changed by hand.
    pub selectable: bool,
    pub items: Vec<NodeView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub name: String,
    pub kind: String,
    /// Latest known measurement, ms. `None` — not measured or the node did not respond.
    pub delay: Option<u32>,
    pub udp: bool,
    /// Nested group: clicking it should go inside, not just select.
    pub is_group: bool,
}

#[tauri::command]
pub async fn get_proxies(state: State<'_, AppState>) -> Result<ProxyOverview> {
    let client = state.client();
    let response = client.proxies().await?;
    Ok(build_overview(response.proxies))
}

#[tauri::command]
pub async fn select_proxy(app: AppHandle, group: String, name: String) -> Result<()> {
    actions::select_proxy(&app, &group, &name).await
}

#[tauri::command]
pub async fn test_group_delay(
    state: State<'_, AppState>,
    group: String,
) -> Result<HashMap<String, u32>> {
    let settings = state.settings.get();
    let client = state.client();
    client
        .group_delay(
            &group,
            &settings.ui.latency_test_url,
            settings.ui.latency_test_timeout,
        )
        .await
}

#[tauri::command]
pub async fn test_proxy_delay(state: State<'_, AppState>, name: String) -> Result<u32> {
    let settings = state.settings.get();
    let client = state.client();
    client
        .proxy_delay(
            &name,
            &settings.ui.latency_test_url,
            settings.ui.latency_test_timeout,
        )
        .await
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_connections(state: State<'_, AppState>) -> Result<ConnectionsSnapshot> {
    let client = state.client();
    client.connections().await
}

#[tauri::command]
pub async fn close_connection(state: State<'_, AppState>, id: String) -> Result<()> {
    let client = state.client();
    client.close_connection(&id).await
}

#[tauri::command]
pub async fn close_all_connections(state: State<'_, AppState>) -> Result<()> {
    let client = state.client();
    client.close_all_connections().await
}

// ---------------------------------------------------------------------------
// sing-box config
// ---------------------------------------------------------------------------

/// Reads the user's `config.json` for the built-in editor.
#[tauri::command]
pub fn read_singbox_config(state: State<'_, AppState>) -> Result<String> {
    let path = config_path(&state)?;
    let content = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
    // Mark the read content as "known": right after opening the editor, the file
    // must not look like it was changed from the outside.
    state.remember_config(&content);
    Ok(content)
}

/// Validates the editor contents without touching the user's file.
///
/// First JSON, then — if the binary is available — a full `sing-box check`
/// on a temporary copy.
#[tauri::command]
pub async fn check_singbox_config(
    state: State<'_, AppState>,
    content: String,
) -> Result<CheckResult> {
    let settings = state.settings.get();
    blocking(move || {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&strip_jsonc(&content)) {
            return Ok(CheckResult {
                available: false,
                ok: false,
                output: format!("invalid JSON: {e}"),
            });
        }

        let scratch = config_dir()?.join("check.json");
        if let Some(parent) = scratch.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        std::fs::write(&scratch, content.as_bytes())
            .map_err(|e| Error::io(scratch.display().to_string(), e))?;

        let choice = binary::resolve(&settings)?;
        let mut result = binary::check_config(&choice.path, &scratch)?;
        let _ = std::fs::remove_file(&scratch);

        if !result.available {
            // JSON was already parsed above, so syntactically everything is fine.
            result.ok = true;
        }
        Ok(result)
    })
    .await
}

/// Writes the editor contents to the user's `config.json`.
/// The previous version stays beside it as `.bak`.
#[tauri::command]
pub async fn write_singbox_config(state: State<'_, AppState>, content: String) -> Result<()> {
    let path = PathBuf::from(config_path(&state)?);
    let body = content.clone();

    blocking(move || {
        if path.is_file() {
            let backup = path.with_extension("json.bak");
            std::fs::copy(&path, &backup)
                .map_err(|e| Error::io(backup.display().to_string(), e))?;
        }

        // Atomically: sing-box may be reading the file at the same moment.
        let tmp = path.with_extension("json.vbtmp");
        std::fs::write(&tmp, body.as_bytes())
            .map_err(|e| Error::io(tmp.display().to_string(), e))?;
        std::fs::rename(&tmp, &path).map_err(|e| Error::io(path.display().to_string(), e))?;
        Ok(())
    })
    .await?;

    state.remember_config(&content);
    Ok(())
}

/// Creates a minimal working `config.json` in the app directory and returns
/// its path — for first-run onboarding. The config has no TUN (no admin rights
/// needed): a local mixed inbound, `direct`/`block`, and a `proxy` selector.
/// We do not write `experimental.clash_api` here — the runtime copy adds it.
///
/// If a file already exists at the default path, we do not overwrite it — just return the path.
#[tauri::command]
pub fn create_minimal_config() -> Result<String> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let path = dir.join("config.json");
    if path.is_file() {
        return Ok(path.display().to_string());
    }

    let minimal = r#"{
  "log": { "level": "info", "timestamp": true },
  "inbounds": [
    { "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": 2080 }
  ],
  "outbounds": [
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" },
    { "type": "selector", "tag": "proxy", "outbounds": ["direct"], "default": "direct" }
  ],
  "route": { "final": "proxy" }
}
"#;

    let tmp = dir.join("config.json.vbtmp");
    std::fs::write(&tmp, minimal.as_bytes())
        .map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(path.display().to_string())
}

fn config_path(state: &State<'_, AppState>) -> Result<String> {
    let path = state.settings.get().sing_box.config_path;
    if path.trim().is_empty() {
        return Err(Error::Other(
            "config.json path is not set — specify it in settings".into(),
        ));
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// sing-box service
//
// The operation bodies live in `actions`: the tray can do the same, and the
// behavior must not diverge between the window and the tray menu.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_run_status(app: AppHandle) -> Result<RunStatus> {
    actions::run_status(&app).await
}

/// Registers the service. The only place where UAC pops up.
#[tauri::command]
pub async fn install_service(app: AppHandle) -> Result<RunStatus> {
    actions::install(&app).await
}

#[tauri::command]
pub async fn uninstall_service(app: AppHandle) -> Result<RunStatus> {
    actions::uninstall(&app).await
}

#[tauri::command]
pub async fn start_service(app: AppHandle) -> Result<RunStatus> {
    actions::start(&app).await
}

#[tauri::command]
pub async fn stop_service(app: AppHandle) -> Result<RunStatus> {
    actions::stop(&app).await
}

#[tauri::command]
pub async fn restart_service(app: AppHandle) -> Result<RestartOutcome> {
    actions::restart(&app).await
}

// ---------------------------------------------------------------------------
// Windows and hotkeys
// ---------------------------------------------------------------------------

/// Hotkeys that could not be acquired: the combination may be taken by another
/// program or contain a typo.
#[tauri::command]
pub fn get_hotkey_problems(state: State<'_, AppState>) -> Vec<String> {
    state
        .hotkey_problems
        .lock()
        .expect("hotkey problems lock")
        .clone()
}

/// The popup closes itself — for example, right after picking a node.
#[tauri::command]
pub fn close_popup(app: AppHandle) {
    crate::window::hide_popup(&app);
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    crate::window::show_main(&app);
}

// ---------------------------------------------------------------------------
// Helpers for the settings form
// ---------------------------------------------------------------------------

/// A new secret for the Clash API. We generate it here, not in the webview: the
/// entropy source in JS is weaker, and the token protects the local control port.
#[tauri::command]
pub fn generate_secret() -> String {
    runtime::generate_secret()
}

/// System file picker dialog. `kind`: `config` or `binary`.
/// `None` — the user closed the dialog, that is not an error.
#[tauri::command]
pub async fn pick_file(app: AppHandle, kind: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;

    let mut builder = app.dialog().file().set_title(if kind == "config" {
        "Choose a sing-box config"
    } else {
        "Choose a sing-box file"
    });

    if kind == "config" {
        builder = builder.add_filter("JSON", &["json", "jsonc"]);
    } else if cfg!(windows) {
        builder = builder.add_filter("Program", &["exe"]);
    }
    builder = builder.add_filter("All files", &["*"]);

    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.pick_file(move |path| {
        let _ = tx.send(path);
    });

    rx.await
        .ok()
        .flatten()
        .and_then(|path| path.into_path().ok())
        .map(|path| path.display().to_string())
}

// ---------------------------------------------------------------------------
// The sing-box file and the version catalog
//
// Each version is downloaded as a separate file and stays on disk. The active
// one is a copy of the selected version at a stable path: the service points to
// it, and switching versions must not require reinstalling it.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_binary_info(state: State<'_, AppState>) -> Result<BinaryInfo> {
    let settings = state.settings.get();
    blocking(move || binary::info(&settings)).await
}

/// Release catalog. By default from cache; `refresh` forces a trip to GitHub.
#[tauri::command]
pub async fn list_singbox_releases(
    state: State<'_, AppState>,
    refresh: bool,
) -> Result<ReleaseCatalog> {
    let active = active_version(&state).await;

    if !refresh {
        return blocking(move || Ok(binary::cached_catalog(active.as_deref()))).await;
    }

    binary::refresh_catalog(active.as_deref()).await
}

/// Downloads a version into the catalog, without switching to it.
#[tauri::command]
pub async fn download_singbox_release(
    state: State<'_, AppState>,
    version: String,
    asset_url: String,
) -> Result<ReleaseCatalog> {
    let target = binary::version_path(&version)?;
    // The name is deliberately unlike a version file: an unfinished download
    // must not end up in the list of downloaded versions.
    let archive = binary::versions_dir()?.join(format!("download-{version}.archive"));

    binary::download(&asset_url, &archive).await?;

    let archive_path = archive.clone();
    blocking(move || {
        let result = binary::extract(&archive_path, &target);
        let _ = std::fs::remove_file(&archive_path);
        result
    })
    .await?;

    let active = active_version(&state).await;
    blocking(move || Ok(binary::cached_catalog(active.as_deref()))).await
}

#[tauri::command]
pub async fn delete_singbox_release(
    state: State<'_, AppState>,
    version: String,
) -> Result<ReleaseCatalog> {
    let active = active_version(&state).await;
    if active.as_deref() == Some(version.as_str()) {
        return Err(Error::Other(
            "this version is currently in use — pick another one first".into(),
        ));
    }

    blocking(move || {
        binary::remove_version(&version)?;
        Ok(binary::cached_catalog(active.as_deref()))
    })
    .await
}

/// Result of switching versions — everything worth showing after it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub binary: BinaryInfo,
    /// Whether sing-box had to be stopped to replace the file.
    pub restarted: bool,
    /// What `sing-box check` said with the new version on the current config.
    pub check: CheckResult,
}

/// Makes a downloaded version active: check the config with it → stop
/// sing-box → replace the file → start it back up.
///
/// Only works on a file managed by Vantage Box: a foreign path is the user's
/// choice, we do not write there.
#[tauri::command]
pub async fn use_singbox_release(
    state: State<'_, AppState>,
    version: String,
) -> Result<InstallOutcome> {
    let settings = state.settings.get();
    let choice = binary::resolve(&settings)?;
    if !choice.managed {
        return Err(Error::Other(
            "the sing-box file path is set manually — Vantage Box does not override it".into(),
        ));
    }

    let source = binary::version_path(&version)?;
    if !source.is_file() {
        return Err(Error::Other(format!(
            "version {version} is not downloaded: {}",
            source.display()
        )));
    }

    let target = choice.path.clone();
    let job_settings = settings.clone();

    let (check, restarted) = blocking(move || {
        // Check the config with the new version specifically: incompatible
        // options must surface before we replace the working file.
        let config = job_settings.sing_box.config_path.trim().to_string();
        let check = if config.is_empty() {
            CheckResult {
                available: false,
                ok: true,
                output: "config path is not set, check skipped".into(),
            }
        } else {
            binary::check_config(&source, std::path::Path::new(&config))?
        };

        if !check.ok {
            return Err(Error::Other(format!(
                "version {version} does not accept the current config, switch cancelled:\n{}",
                check.output
            )));
        }

        let was_running = process::running()
            || service::status().map(|info| info.is_running()).unwrap_or(false);
        if was_running {
            process::stop()?;
            if service::status()
                .map(|info| info.state != crate::service::ServiceState::NotInstalled)
                .unwrap_or(false)
            {
                service::stop()?;
            }
        }

        replace_binary(&source, &target)?;

        if was_running {
            if service::status()
                .map(|info| info.state != crate::service::ServiceState::NotInstalled)
                .unwrap_or(false)
            {
                runtime::prepare(&job_settings)?;
                service::start()?;
            } else {
                process::start(&job_settings)?;
            }
        }

        Ok((check, was_running))
    })
    .await?;

    if restarted {
        state::reconnect(&state)?;
    }

    let info_settings = state.settings.get();
    let binary = blocking(move || binary::info(&info_settings)).await?;

    Ok(InstallOutcome {
        binary,
        restarted,
        check,
    })
}

/// Version of the active sing-box file. `None` — the file is missing or the version is unreadable.
async fn active_version(state: &State<'_, AppState>) -> Option<String> {
    let settings = state.settings.get();
    blocking(move || binary::info(&settings))
        .await
        .ok()
        .and_then(|info| info.version)
}

/// Makes a file active, moving the previous one aside: on Windows you cannot
/// overwrite an executable still held by someone, but you can rename it.
///
/// The source is copied, not moved: it is a file from the version catalog, it
/// must stay on disk.
fn replace_binary(source: &std::path::Path, target: &std::path::Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }

    let previous = target.with_extension("old");
    let _ = std::fs::remove_file(&previous);

    if target.exists() {
        std::fs::rename(target, &previous)
            .map_err(|e| Error::io(target.display().to_string(), e))?;
    }

    match std::fs::copy(source, target) {
        Ok(_) => {
            let _ = std::fs::remove_file(&previous);
            Ok(())
        }
        Err(e) => {
            // Roll back so we are not left without a sing-box file at all.
            if previous.exists() {
                let _ = std::fs::rename(&previous, target);
            }
            Err(Error::io(target.display().to_string(), e))
        }
    }
}

fn build_overview(proxies: HashMap<String, Proxy>) -> ProxyOverview {
    // Take the group order from GLOBAL, if sing-box returned it: it reflects
    // the outbound order in the config, not a random hash-table traversal.
    let global_order: Vec<String> = proxies
        .get("GLOBAL")
        .and_then(|p| p.all.clone())
        .unwrap_or_default();

    // Take the name from the map key: sing-box does not always return the
    // `name` field inside the object, but the key is always there.
    let mut groups: Vec<GroupView> = proxies
        .iter()
        .filter(|(name, p)| p.is_group() && name.as_str() != "GLOBAL")
        .map(|(group_name, group)| {
            let items = group
                .all
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|name| {
                    let node = proxies.get(&name);
                    NodeView {
                        delay: node
                            .and_then(|n| n.history.last())
                            .map(|h| h.delay)
                            // Zero means "the node did not respond", not an instant reply.
                            .filter(|d| *d > 0),
                        kind: node.map(|n| n.kind.clone()).unwrap_or_default(),
                        udp: node.is_some_and(|n| n.udp),
                        is_group: node.is_some_and(|n| n.is_group()),
                        name,
                    }
                })
                .collect();

            GroupView {
                name: group_name.clone(),
                kind: group.kind.clone(),
                now: group.now.clone(),
                selectable: group.is_selectable(),
                items,
            }
        })
        .collect();

    // Order from GLOBAL, everything else alphabetically after it. Without the
    // second key the order would drift from call to call: the source is a HashMap.
    groups.sort_by_key(|g| {
        let rank = global_order
            .iter()
            .position(|n| n == &g.name)
            .unwrap_or(usize::MAX);
        (rank, g.name.clone())
    });

    ProxyOverview { groups }
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

/// Pulls all enabled subscriptions and injects nodes into config.json. `force`
/// ignores the set signature — for example, when the user clicks "refresh".
#[tauri::command]
pub async fn refresh_subscriptions(app: AppHandle, force: bool) -> Result<ApplyOutcome> {
    subscription::apply(&app, force).await
}

/// Subscription state from the sidecar file: last update time, node count,
/// errors — for display in the UI.
#[tauri::command]
pub fn get_subscription_state() -> Result<SubscriptionsState> {
    subscription::load_state()
}
