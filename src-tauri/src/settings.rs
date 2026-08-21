//! `settings.json` — the single source of truth for GUI configuration.
//!
//! The file lives in the standard OS settings directory, is read as JSONC,
//! written atomically, and watched via `notify`: manual edits in an editor
//! are picked up on the fly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;

/// The app directory name inside the system config folder.
const APP_DIR: &str = "vantage-box";
const FILE_NAME: &str = "settings.json";

/// The window over which consecutive filesystem events are collapsed.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Settings model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// A link to the JSON Schema — so editors provide autocompletion.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub sing_box: SingBoxSettings,
    pub clash_api: ClashApiSettings,
    pub ui: UiSettings,
    pub tray: TraySettings,
    pub hotkeys: HotkeySettings,
    /// Start Vantage Box at system login.
    pub autostart: bool,
    /// Auto-update of the GUI itself (not sing-box). Separate from `sing_box.update_policy`.
    pub gui_update: GuiUpdateSettings,
    /// Subscriptions to proxy lists.
    pub subscriptions: Vec<SubscriptionSettings>,
    /// Auto-switch selector groups to a backup when the active node fails.
    pub fallback: FallbackSettings,
}

/// GUI auto-update policy. Reuses the same values as
/// `sing_box.update_policy`, but lives separately: the binary and the shell
/// are updated independently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GuiUpdateSettings {
    pub policy: UpdatePolicy,
}

/// One subscription: a URL that returns a proxy list, and where to inject it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubscriptionSettings {
    /// A unique identifier within the list. Not provided by the upstream —
    /// we generate it ourselves so the UI and backend can refer to a subscription stably.
    pub id: String,
    /// A human-readable name.
    pub name: String,
    /// The subscription URL.
    pub url: String,
    /// Whether the subscription is enabled (a disabled one is not updated or injected).
    pub enabled: bool,
    /// The selector group tag whose `outbounds` to append nodes to. `None` — into all
    /// selector/urltest groups.
    pub target_group: Option<String>,
    /// How often to pull the subscription, hours.
    pub update_interval: u64,
}

/// Auto-switching selector groups: when the active node goes down, pick a backup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FallbackSettings {
    pub enabled: bool,
    /// How often we ping the active node, seconds.
    pub interval_sec: u32,
    /// Ping timeout, ms.
    pub timeout_ms: u32,
    /// A delay above this is considered "bad" — switch. 0 — only on full
    /// unavailability.
    pub max_delay_ms: u32,
    /// Group tags to watch. Empty — all selector groups.
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SingBoxSettings {
    /// Path to the user's sing-box `config.json`.
    pub config_path: String,
    /// Path to the binary. Empty — use the binary managed by Vantage Box.
    pub binary_path: String,
    /// Binary auto-update policy.
    pub update_policy: UpdatePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePolicy {
    Off,
    Notify,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClashApiSettings {
    /// Base address of the Clash API. Strictly loopback.
    pub url: String,
    /// Secret from the sing-box config. Empty — if the API is open with no auth.
    pub secret: String,
    /// The level at which we subscribe to `/logs`.
    pub log_level: LogLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiSettings {
    pub theme: Theme,
    /// The URL used to measure outbound latency.
    pub latency_test_url: String,
    /// Latency test timeout, ms.
    pub latency_test_timeout: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TraySettings {
    pub enabled: bool,
    /// Closing the window minimizes to the tray instead of quitting.
    pub close_to_tray: bool,
    pub start_minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HotkeySettings {
    // --- Global: work even when the window is closed ---
    /// Proxy selection popup.
    pub proxy_popup: String,
    /// Toggle sing-box on/off.
    pub toggle: String,
    /// Show and focus the main window.
    pub show_main: String,
    /// Soft restart of the current run (preserves selector selections).
    pub restart: String,
    // --- In-app: only while the main window is focused ---
    /// Jump to the Settings tab.
    pub go_to_settings: String,
    /// Cycle to the next tab.
    pub next_tab: String,
    /// Cycle to the previous tab.
    pub prev_tab: String,
    /// Modifier prefix for "jump to tab by index": digits 1–9 are appended at
    /// runtime, so `"Ctrl"` binds `Ctrl+1`…`Ctrl+9`.
    pub tab_index: String,
    /// Close the window (goes through the close-to-tray handler).
    pub close_window: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: Some("./settings.schema.json".into()),
            sing_box: SingBoxSettings::default(),
            clash_api: ClashApiSettings::default(),
            ui: UiSettings::default(),
            tray: TraySettings::default(),
            hotkeys: HotkeySettings::default(),
            autostart: false,
            gui_update: GuiUpdateSettings::default(),
            subscriptions: Vec::new(),
            fallback: FallbackSettings::default(),
        }
    }
}

impl Default for GuiUpdateSettings {
    fn default() -> Self {
        Self {
            policy: UpdatePolicy::Notify,
        }
    }
}

impl Default for SubscriptionSettings {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            enabled: true,
            target_group: None,
            update_interval: 24,
        }
    }
}

impl Default for FallbackSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_sec: 60,
            timeout_ms: 5000,
            max_delay_ms: 0,
            groups: Vec::new(),
        }
    }
}

impl Default for SingBoxSettings {
    fn default() -> Self {
        Self {
            config_path: String::new(),
            binary_path: String::new(),
            update_policy: UpdatePolicy::Notify,
        }
    }
}

/// The default Clash API address.
///
/// Port 9090 is the common standard for all Clash-based clients, and also the
/// standard Prometheus port. On a machine already running one of those, two
/// processes would fight over one port. Our own port removes the question entirely.
pub const DEFAULT_CLASH_URL: &str = "http://127.0.0.1:9797";

/// The port that all other Clash clients use.
const SHARED_CLASH_PORT: &str = ":9090";

/// Brings settings to a working form after reading from disk.
///
/// The only rule: move the standard Clash port to our own. This is not a taste
/// call — on 9090 the app may well connect to a different sing-box than the
/// one it started itself.
fn normalize(mut settings: Settings) -> Settings {
    if settings
        .clash_api
        .url
        .trim()
        .trim_end_matches('/')
        .ends_with(SHARED_CLASH_PORT)
    {
        settings.clash_api.url = DEFAULT_CLASH_URL.into();
    }
    settings
}

impl Default for ClashApiSettings {
    fn default() -> Self {
        Self {
            url: DEFAULT_CLASH_URL.into(),
            secret: String::new(),
            log_level: LogLevel::Info,
        }
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            latency_test_url: "http://www.gstatic.com/generate_204".into(),
            latency_test_timeout: 5000,
        }
    }
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            close_to_tray: true,
            start_minimized: false,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            // Global
            proxy_popup: "Ctrl+Alt+P".into(),
            toggle: "Ctrl+Alt+O".into(),
            show_main: "Ctrl+Alt+V".into(),
            restart: "Ctrl+Alt+R".into(),
            // In-app
            go_to_settings: "Ctrl+Alt+S".into(),
            next_tab: "Ctrl+Tab".into(),
            prev_tab: "Ctrl+Shift+Tab".into(),
            tab_index: "Ctrl".into(),
            close_window: "Ctrl+Shift+W".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// An environment variable that moves all app state to a different folder.
/// Integration tests need it so they definitely do not touch the user's
/// working configuration.
pub const CONFIG_DIR_ENV: &str = "VANTAGE_BOX_CONFIG_DIR";

/// The settings directory: `%APPDATA%/vantage-box`, `~/.config/vantage-box`,
/// `~/Library/Application Support/vantage-box`.
pub fn config_dir() -> Result<PathBuf> {
    if let Some(custom) = std::env::var_os(CONFIG_DIR_ENV) {
        if !custom.is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }

    dirs::config_dir()
        .map(|d| d.join(APP_DIR))
        .ok_or(Error::NoConfigDir)
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(FILE_NAME))
}

// ---------------------------------------------------------------------------
// Reading and writing
// ---------------------------------------------------------------------------

/// Reads settings from disk. A missing file is not an error: we take the
/// defaults and create the file so the user has something to edit.
pub fn load_or_create() -> Result<Settings> {
    let path = settings_path()?;
    if !path.exists() {
        let defaults = Settings::default();
        write_to_disk(&path, &defaults)?;
        write_schema()?;
        return Ok(defaults);
    }

    let settings = read_from_disk(&path)?;
    // If reading changed anything (for example, moved the port off the shared 9090),
    // the file must reflect it right away: the user looks at exactly that file.
    if let Ok(raw) = read_raw(&path) {
        if raw != settings {
            write_to_disk(&path, &settings)?;
        }
    }
    Ok(settings)
}

fn read_raw(path: &Path) -> Result<Settings> {
    let display = path.display().to_string();
    let raw = std::fs::read_to_string(path).map_err(|e| Error::io(&display, e))?;
    if raw.trim().is_empty() {
        return Ok(Settings::default());
    }
    serde_json::from_str(&strip_jsonc(&raw)).map_err(|e| Error::parse(&display, e))
}

pub fn read_from_disk(path: &Path) -> Result<Settings> {
    // An empty file is treated as "all defaults" — otherwise the very first
    // `truncate` from an editor would kill loading.
    read_raw(path).map(normalize)
}

/// Atomic write: we write to a temp file next to the target and rename over it.
/// That way the editor/watcher never sees a half-empty file.
pub fn write_to_disk(path: &Path, settings: &Settings) -> Result<()> {
    let display = path.display().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let mut body = serde_json::to_string_pretty(settings).map_err(|e| Error::parse(&display, e))?;
    body.push('\n');

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(&display, e))?;
    Ok(())
}

/// Writes the JSON Schema next to the settings — `$schema` points to it, so
/// autocompletion works in any editor offline.
pub fn write_schema() -> Result<()> {
    let path = config_dir()?.join("settings.schema.json");
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    std::fs::write(&path, include_str!("../schemas/settings.schema.json"))
        .map_err(|e| Error::io(path.display().to_string(), e))
}

// ---------------------------------------------------------------------------
// In-memory store + watcher
// ---------------------------------------------------------------------------

/// Current settings, shared between commands and background tasks.
pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    pub fn new(path: PathBuf, initial: Settings) -> Self {
        Self {
            path,
            current: RwLock::new(initial),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.read().expect("settings lock").clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes to disk and updates memory. The watcher will see its own write,
    /// but the comparison with the in-memory value will suppress it.
    pub fn save(&self, next: Settings) -> Result<()> {
        write_to_disk(&self.path, &next)?;
        *self.current.write().expect("settings lock") = next;
        Ok(())
    }

    /// Re-reads the file. `Ok(None)` — the contents have not changed.
    pub fn reload(&self) -> Result<Option<Settings>> {
        let next = read_from_disk(&self.path)?;
        let mut guard = self.current.write().expect("settings lock");
        if *guard == next {
            return Ok(None);
        }
        *guard = next.clone();
        Ok(Some(next))
    }
}

/// Event from the watcher: the file changed and its contents are actually different.
pub type ChangeTx = tokio::sync::mpsc::UnboundedSender<()>;

/// Watches the settings file. We attach the watcher to the *directory*: an
/// atomic write replaces the inode, so a subscription on the file itself dies
/// after the first write.
///
/// The returned watcher must be kept alive — on drop the watch stops.
pub fn spawn_watcher(path: &Path, tx: ChangeTx) -> Result<notify::RecommendedWatcher> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Other("the settings file has no parent directory".into()))?
        .to_path_buf();
    let target = path.to_path_buf();

    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<()>();

    let mut watcher =
        notify::recommended_watcher(move |res: std::result::Result<Event, notify::Error>| {
            let Ok(event) = res else { return };
            // The temp file of the atomic write is not interesting.
            let touches_target = event.paths.iter().any(|p| p == &target);
            if touches_target {
                let _ = raw_tx.send(());
            }
        })
        .map_err(|e| Error::Other(format!("failed to create watcher: {e}")))?;

    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| Error::Other(format!("failed to watch {}: {e}", dir.display())))?;

    // Debounce on a separate thread: saving from an editor usually produces
    // several events in a row (truncate, write, rename).
    std::thread::spawn(move || {
        while raw_rx.recv().is_ok() {
            // Collapse everything that arrived within the debounce window.
            while raw_rx.recv_timeout(WATCH_DEBOUNCE).is_ok() {}
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    Ok(watcher)
}

pub type SharedSettings = Arc<SettingsStore>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_off_shared_clash_port() {
        let mut settings = Settings::default();
        settings.clash_api.url = "http://127.0.0.1:9090".into();
        assert_eq!(normalize(settings).clash_api.url, DEFAULT_CLASH_URL);

        let mut settings = Settings::default();
        settings.clash_api.url = "http://127.0.0.1:9090/".into();
        assert_eq!(normalize(settings).clash_api.url, DEFAULT_CLASH_URL);
    }

    #[test]
    fn keeps_deliberate_port() {
        let mut settings = Settings::default();
        settings.clash_api.url = "http://127.0.0.1:18080".into();
        assert_eq!(normalize(settings).clash_api.url, "http://127.0.0.1:18080");
    }
}
