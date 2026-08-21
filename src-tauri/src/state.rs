//! Global application state.
//!
//! Exactly two things live here: settings and the connection to the Clash API.
//! We do not cache the sing-box runtime state — the source of truth is always
//! sing-box itself.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::clash::{ClashClient, StreamManager};
use crate::error::Result;
use crate::settings::{ChangeTx, Settings, SettingsStore, SharedSettings};

fn digest(content: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.finalize().into()
}

/// Event for the UI: `settings.json` changed (by us or by hand in an editor).
pub const EVENT_SETTINGS: &str = "settings://changed";
/// Event for the UI: `settings.json` cannot be read (broken JSON, etc.).
pub const EVENT_SETTINGS_ERROR: &str = "settings://error";
/// Event for the UI: the user's `config.json` changed externally.
pub const EVENT_CONFIG_CHANGED: &str = "singbox://config-changed";

pub struct AppState {
    pub settings: SharedSettings,
    /// Rebuilt when the `clashApi` section changes.
    pub client: RwLock<ClashClient>,
    pub streams: Arc<StreamManager>,
    /// The watcher must live as long as the application: on drop, the watch on
    /// the settings file silently stops.
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// A separate watch on the sing-box `config.json`: its path changes with
    /// the settings, so the watcher has to be re-armed.
    pub config_watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub config_watch_tx: ChangeTx,
    /// Hash of the last contents we wrote. Needed so our own write does not
    /// look like an external edit of the file.
    pub config_signature: Mutex<Option<[u8; 32]>>,
    /// Hotkeys that failed to register, with reasons.
    pub hotkey_problems: Mutex<Vec<String>>,
}

impl AppState {
    pub fn new(
        settings: SharedSettings,
        client: ClashClient,
        streams: Arc<StreamManager>,
        config_watch_tx: ChangeTx,
    ) -> Self {
        Self {
            settings,
            client: RwLock::new(client),
            streams,
            watcher: Mutex::new(None),
            config_watcher: Mutex::new(None),
            config_watch_tx,
            config_signature: Mutex::new(None),
            hotkey_problems: Mutex::new(Vec::new()),
        }
    }

    /// Re-arms the watch on the current `config.json`. An empty path takes it down.
    pub fn rearm_config_watcher(&self, path: &str) {
        let path = path.trim();
        let mut slot = self.config_watcher.lock().expect("config watcher lock");

        if path.is_empty() {
            *slot = None;
            return;
        }

        match crate::settings::spawn_watcher(Path::new(path), self.config_watch_tx.clone()) {
            Ok(watcher) => *slot = Some(watcher),
            Err(e) => {
                *slot = None;
                eprintln!("config.json watcher not started: {e}");
            }
        }
    }

    /// Records that the file currently contains exactly this.
    pub fn remember_config(&self, content: &str) {
        *self.config_signature.lock().expect("signature lock") = Some(digest(content));
    }

    /// `true` if the file contents differ from the last thing we wrote.
    pub fn config_changed_externally(&self, path: &str) -> bool {
        let Ok(content) = std::fs::read_to_string(path) else {
            // The file disappeared or became unreadable — definitely worth reporting.
            return true;
        };
        let known = *self.config_signature.lock().expect("signature lock");
        known != Some(digest(&content))
    }

    /// A snapshot of the client for use in async commands: holding a guard
    /// across `.await` is not allowed, so we clone (inside `reqwest::Client`
    /// a clone is cheap and reuses the connection pool).
    pub fn client(&self) -> ClashClient {
        self.client.read().expect("client lock").clone()
    }
}

/// Applies new settings to the runtime: rebuilds the client and, if the
/// connection or log level changed, restarts the subscriptions.
pub fn apply_settings(
    app: &AppHandle,
    state: &AppState,
    previous: &Settings,
    next: &Settings,
) -> Result<()> {
    if previous.sing_box.config_path != next.sing_box.config_path {
        state.rearm_config_watcher(&next.sing_box.config_path);
    }

    if previous.hotkeys != next.hotkeys {
        crate::hotkeys::apply(app, next);
    }

    if previous.autostart != next.autostart {
        sync_autostart(app, next.autostart);
    }

    let connection_changed = previous.clash_api != next.clash_api;

    if connection_changed {
        let client = ClashClient::new(&crate::runtime::effective_api_settings(next))?;
        *state.client.write().expect("client lock") = client.clone();
        state.streams.restart(client, next.clash_api.log_level);
    }

    let _ = app.emit(EVENT_SETTINGS, next.clone());
    Ok(())
}

/// Rebuilds the client and subscriptions forcibly — for the "reconnect" button.
pub fn reconnect(state: &AppState) -> Result<()> {
    let settings = state.settings.get();
    let client = ClashClient::new(&crate::runtime::effective_api_settings(&settings))?;
    *state.client.write().expect("client lock") = client.clone();
    state.streams.restart(client, settings.clash_api.log_level);
    Ok(())
}

/// Brings autostart in line with the settings.
///
/// We ask the system for the real state: the user may have removed the entry
/// from autostart outside the app, and then our flag would lie.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();
    if manager.is_enabled().unwrap_or(false) == enabled {
        return;
    }

    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    if let Err(e) = result {
        eprintln!("failed to change autostart: {e}");
    }
}

pub fn share(store: SettingsStore) -> SharedSettings {
    Arc::new(store)
}
