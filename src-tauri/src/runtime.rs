//! Preparing the runtime copy of the sing-box config.
//!
//! We never overwrite the user's `config.json`. Before starting the service,
//! `runtime.json` appears next to the settings — the same configuration, but
//! with Clash API guaranteed to be on and the secret injected.
//! The secret does not live in `settings.json`: it is generated on the fly
//! and stored in a separate `runtime-state.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;
use crate::settings::{config_dir, ClashApiSettings, Settings};

const RUNTIME_CONFIG: &str = "runtime.json";
const RUNTIME_STATE: &str = "runtime-state.json";

/// What came out of preparation — this is what sing-box is started with.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedConfig {
    /// Path to the runtime copy, passed to sing-box via `-c`.
    pub config_path: String,
    /// `host:port` where the Clash API will listen.
    pub external_controller: String,
    /// Whether the secret came from the user's config (then we left it alone).
    pub secret_from_user_config: bool,
    /// The effective secret. Not exposed: the struct goes to the frontend, and
    /// showing the control token there serves no purpose.
    #[serde(skip)]
    pub secret: String,
}

/// What the GUI needs to know after the service starts, but is not in settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuntimeState {
    pub api_url: String,
    pub secret: String,
}

pub fn runtime_config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(RUNTIME_CONFIG))
}

fn runtime_state_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(RUNTIME_STATE))
}

/// Builds the runtime config from the user's one and writes it to disk.
pub fn prepare(settings: &Settings) -> Result<PreparedConfig> {
    prepare_in(&config_dir()?, settings)
}

/// Same, but with an explicit directory for the runtime files.
///
/// The separate parameter is needed by tools that run several sing-box
/// versions in a row: each needs its own sandbox, and swapping an environment
/// variable on the fly for this is not the right approach.
pub fn prepare_in(dir: &Path, settings: &Settings) -> Result<PreparedConfig> {
    let source = settings.sing_box.config_path.trim();
    if source.is_empty() {
        return Err(Error::Other(
            "sing-box config.json path is not set — specify it in settings".into(),
        ));
    }

    let raw = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let mut config: Value = serde_json::from_str(&strip_jsonc(&raw))
        .map_err(|e| Error::parse(source, e))?;

    if !config.is_object() {
        return Err(Error::Other(format!(
            "{source}: expected a JSON object at the root of the config"
        )));
    }

    let external_controller = host_port(&settings.clash_api.url);

    // A secret already set by the user is their decision — we respect and reuse it.
    let existing_secret = config
        .pointer("/experimental/clash_api/secret")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let secret_from_user_config = existing_secret.is_some();
    let secret = match existing_secret {
        Some(secret) => secret,
        None if !settings.clash_api.secret.is_empty() => settings.clash_api.secret.clone(),
        None => generate_secret(),
    };

    // Without Clash API the app is blind, so we bring this block to a working
    // state regardless of what was in the original config. The other clash_api
    // keys (external_ui, default_mode, etc.) are preserved as-is.
    let root = config.as_object_mut().expect("checked above");
    let experimental = root.entry("experimental").or_insert_with(|| json!({}));
    if !experimental.is_object() {
        *experimental = json!({});
    }
    let clash_api = experimental
        .as_object_mut()
        .expect("just converted to an object")
        .entry("clash_api")
        .or_insert_with(|| json!({}));
    if !clash_api.is_object() {
        *clash_api = json!({});
    }
    clash_api["external_controller"] = json!(external_controller);
    clash_api["secret"] = json!(secret.clone());

    let path = dir.join(RUNTIME_CONFIG);
    write_private(&path, &serde_json::to_vec_pretty(&config).unwrap_or_default())?;

    write_private(
        &dir.join(RUNTIME_STATE),
        &serde_json::to_vec_pretty(&RuntimeState {
            api_url: settings.clash_api.url.clone(),
            secret: secret.clone(),
        })
        .unwrap_or_default(),
    )?;

    Ok(PreparedConfig {
        config_path: path.display().to_string(),
        external_controller,
        secret_from_user_config,
        secret,
    })
}

/// Whether the config needs a TUN inbound.
///
/// This determines whether the service is required: TUN brings up a network
/// adapter and requires admin rights, everything else runs as a regular process.
pub fn requires_tun(settings: &Settings) -> Result<bool> {
    let source = settings.sing_box.config_path.trim();
    if source.is_empty() {
        return Err(Error::Other(
            "sing-box config.json path is not set — specify it in settings".into(),
        ));
    }

    let raw = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let config: Value =
        serde_json::from_str(&strip_jsonc(&raw)).map_err(|e| Error::parse(source, e))?;

    Ok(config
        .get("inbounds")
        .and_then(Value::as_array)
        .is_some_and(|inbounds| {
            inbounds
                .iter()
                .any(|inbound| inbound.get("type").and_then(Value::as_str) == Some("tun"))
        }))
}

pub fn load_state() -> RuntimeState {
    let Ok(path) = runtime_state_path() else {
        return RuntimeState::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Connection settings with the effective secret already injected.
///
/// An explicit value from `settings.json` takes priority: if the user set it,
/// they expect exactly that. Otherwise we take what we started sing-box with —
/// but only if the API address has not changed since.
pub fn effective_api_settings(settings: &Settings) -> ClashApiSettings {
    if !settings.clash_api.secret.is_empty() {
        return settings.clash_api.clone();
    }

    let state = load_state();
    if state.secret.is_empty() || state.api_url != settings.clash_api.url {
        return settings.clash_api.clone();
    }

    ClashApiSettings {
        secret: state.secret,
        ..settings.clash_api.clone()
    }
}

/// Files with the secret are written owner-only. On Windows this is already
/// ensured by the user profile ACL; on Unix the mode must be set explicitly.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    std::fs::write(path, body).map_err(|e| Error::io(path.display().to_string(), e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// 16 random bytes in hex. We use the system CSPRNG — the secret protects the
/// local control port, weak entropy is not acceptable here.
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // The only failure scenario is an unavailable system entropy source.
        // Then it is more honest not to inject a predictable value and to
        // leave the API without auth: it is loopback-only anyway.
        return String::new();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `http://127.0.0.1:9090/` → `127.0.0.1:9090`.
fn host_port(url: &str) -> String {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let host = without_scheme
        .split(['/', '?'])
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('/');
    if host.is_empty() {
        "127.0.0.1:9090".into()
    } else {
        host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_host_port() {
        assert_eq!(host_port("http://127.0.0.1:9090"), "127.0.0.1:9090");
        assert_eq!(host_port("http://127.0.0.1:9090/"), "127.0.0.1:9090");
        assert_eq!(host_port("127.0.0.1:9090"), "127.0.0.1:9090");
        assert_eq!(host_port(""), "127.0.0.1:9090");
    }

    /// A per-test sandbox: a user config and a runtime directory.
    fn sandbox(name: &str, config: &str) -> (PathBuf, Settings) {
        let dir = std::env::temp_dir().join(format!("vantage-box-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("config.json");
        std::fs::write(&config_path, config).unwrap();

        let mut settings = Settings::default();
        settings.sing_box.config_path = config_path.display().to_string();
        (dir, settings)
    }

    #[test]
    fn detects_tun_inbound() {
        let (_, with_tun) = sandbox(
            "tun",
            r#"{"inbounds":[{"type":"tun","tag":"tun-in"}],"outbounds":[]}"#,
        );
        assert!(requires_tun(&with_tun).unwrap());

        let (_, without_tun) = sandbox(
            "no-tun",
            r#"{"inbounds":[{"type":"mixed","listen_port":2080}]}"#,
        );
        assert!(!requires_tun(&without_tun).unwrap());

        let (_, empty) = sandbox("no-inbounds", r#"{"outbounds":[]}"#);
        assert!(!requires_tun(&empty).unwrap());
    }

    /// The user's config must stay recognizable: the key order is the same,
    /// and our block is appended at the end — so line numbers in sing-box
    /// errors match the original file as closely as possible.
    #[test]
    fn keeps_key_order_and_appends_our_block() {
        let (dir, settings) = sandbox(
            "order",
            r#"{"log":{"level":"info"},"dns":{},"inbounds":[],"outbounds":[]}"#,
        );

        prepare_in(&dir, &settings).unwrap();
        let written = std::fs::read_to_string(dir.join(RUNTIME_CONFIG)).unwrap();

        let keys: Vec<&str> = ["log", "dns", "inbounds", "outbounds", "experimental"]
            .into_iter()
            .filter(|key| written.contains(&format!("\"{key}\"")))
            .collect();
        assert_eq!(keys, ["log", "dns", "inbounds", "outbounds", "experimental"]);
        assert!(written.find("\"log\"").unwrap() < written.find("\"experimental\"").unwrap());
    }

    /// An already-set external_controller is patched in place, not added a second time.
    #[test]
    fn replaces_existing_field_in_place() {
        let (dir, settings) = sandbox(
            "in-place",
            r#"{"experimental":{"clash_api":{"external_controller":"127.0.0.1:1","external_ui":"ui"}},"log":{}}"#,
        );

        let prepared = prepare_in(&dir, &settings).unwrap();
        let written = std::fs::read_to_string(dir.join(RUNTIME_CONFIG)).unwrap();

        assert!(written.contains(&prepared.external_controller));
        assert!(!written.contains("127.0.0.1:1\""));
        // We do not touch neighboring keys: those are the user's settings.
        assert!(written.contains("external_ui"));
        // The block stayed first — there is no reason to move it down.
        assert!(written.find("\"experimental\"").unwrap() < written.find("\"log\"").unwrap());
    }

    #[test]
    fn generates_distinct_secrets() {
        let a = generate_secret();
        let b = generate_secret();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
