//! The `vantage://` URI scheme — a fire-and-forget way for web pages, Raycast,
//! and shortcuts to ask the running app to do something.
//!
//! The OS launches `vantage-box.exe "uri" "<url>"`. When the app is already
//! running, `tauri-plugin-single-instance` routes the second launch's args to
//! the first instance's callback; when not, the same binary starts normally and
//! `setup()` finds the URI in `argv`. Both paths funnel into [`dispatch`].
//!
//! Security posture: the web is an untrusted source. A fixed whitelist of
//! low-risk actions is accepted; state-changing ones require an in-app
//! confirmation dialog. Anything that takes a path, a config blob, or admin
//! rights (`install`/`uninstall`) is rejected outright — a URI that could
//! install a service or overwrite config would be a phishing vector.

use std::collections::HashMap;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::actions;
use crate::window;

/// The argv token that marks a URI launch, e.g. `vantage-box.exe "uri" "vantage://toggle"`.
pub const FLAG: &str = "uri";

/// A parsed, validated URI action. Construct only via [`parse`], which enforces
/// the whitelist and parameter bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriAction {
    Start,
    Stop,
    Toggle,
    /// Bring the main window to the front — the only no-confirm action.
    Show,
    Select {
        group: String,
        name: String,
    },
}

impl UriAction {
    /// Human label for the confirmation dialog and logs.
    fn label(&self) -> String {
        match self {
            Self::Start => "start".into(),
            Self::Stop => "stop".into(),
            Self::Toggle => "toggle".into(),
            Self::Show => "show".into(),
            Self::Select { group, name } => format!("select {group} → {name}"),
        }
    }

    /// `Show` is safe to run without a prompt; everything else changes tunnel
    /// state and must be confirmed.
    fn needs_confirm(&self) -> bool {
        !matches!(self, Self::Show)
    }
}

/// Pull the URI out of an argv stream: the token after `FLAG`. Returns `None`
/// when this is not a URI launch (no `FLAG` present, or `FLAG` is the last arg).
pub fn extract_uri(args: impl Iterator<Item = String>) -> Option<String> {
    let mut args = args;
    args.find(|a| a == FLAG)?;
    args.next()
}

/// Convenience for cold-start: read `std::env::args`.
pub fn cold_start_uri() -> Option<String> {
    extract_uri(std::env::args())
}

/// Parse and validate a `vantage://` URI into an action, or reject with a
/// human-readable reason (the caller logs and ignores — there is no client to
/// send a JSON-RPC error to).
pub fn parse(uri: &str) -> Result<UriAction, String> {
    let rest = uri
        .strip_prefix("vantage://")
        .ok_or("not a vantage:// URI")?;
    let (action_part, query) = match rest.split_once('?') {
        Some((a, q)) => (a, q),
        None => (rest, ""),
    };
    // The action is the leading path segment, case-insensitive.
    let action = action_part
        .split('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let query = parse_query(query);

    match action.as_str() {
        "start" => Ok(UriAction::Start),
        "stop" => Ok(UriAction::Stop),
        "toggle" => Ok(UriAction::Toggle),
        // `status` has no useful side effect over the bus; in the URI surface
        // it is just "bring the app forward" (MVP).
        "show" | "status" => Ok(UriAction::Show),
        "select" => {
            let group = query.get("group").cloned().unwrap_or_default();
            // Accept `node` (preferred) and `name` (lenient) for the selected item.
            let name = query
                .get("node")
                .or_else(|| query.get("name"))
                .cloned()
                .unwrap_or_default();
            if !valid_token(&group) || !valid_token(&name) {
                return Err(
                    "select requires non-empty group and node (≤256 chars, no path separators)"
                        .into(),
                );
            }
            Ok(UriAction::Select { group, name })
        }
        other => Err(format!("unknown action: {other}")),
    }
}

/// Execute the action. The URI surface is fire-and-forget: web pages get no
/// response channel, so failures are logged and swallowed. State-changing
/// actions prompt the user; dismissing the dialog is a silent no-op.
pub async fn dispatch(handle: &AppHandle, uri: &str) {
    let action = match parse(uri) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("vantage-box uri: ignored ({e}): {uri}");
            return;
        }
    };

    // Compute the label up front: `match action` below moves `action`, so we
    // cannot call `action.label()` in the final error log.
    let label = action.label();

    if action.needs_confirm() {
        // The dialog API here is callback-based (`show`), not a future. Bridge
        // it with a oneshot so we can await the user's choice.
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        handle
            .dialog()
            .message(format!("An external link wants to: {label}.\n\nAllow?"))
            .title("Vantage Box")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Allow".to_string(),
                "Cancel".to_string(),
            ))
            .show(move |yes| {
                let _ = tx.send(yes);
            });
        let approved = rx.await.unwrap_or(false);
        if !approved {
            eprintln!("vantage-box uri: dismissed by user");
            return;
        }
    }

    let outcome = match action {
        UriAction::Show => {
            window::show_main(handle);
            Ok(())
        }
        UriAction::Start => actions::start(handle).await.map(|_| ()),
        UriAction::Stop => actions::stop(handle).await.map(|_| ()),
        UriAction::Toggle => actions::toggle(handle).await.map(|_| ()),
        UriAction::Select { group, name } => actions::select_proxy(handle, &group, &name)
            .await
            .map(|_| ()),
    };

    if let Err(e) = outcome {
        eprintln!("vantage-box uri: {label} failed: {e}");
    }
}

// -- parsing helpers ------------------------------------------------------------

fn parse_query(q: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        map.insert(k.to_string(), percent_decode(v));
    }
    map
}

/// Minimal percent-decoding for query values. `url` is not a dependency, and
/// the only values we accept are group/node names.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// A safe group/node token: non-empty, bounded, no path separators, no control
/// characters (including NUL). Path separators are rejected even when they
/// arrive percent-encoded — a node name containing `/` is not a real node.
fn valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.contains('/')
        && !s.contains('\\')
        && s.chars().all(|c| !c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_actions() {
        assert_eq!(parse("vantage://start").unwrap(), UriAction::Start);
        assert_eq!(parse("vantage://stop").unwrap(), UriAction::Stop);
        assert_eq!(parse("vantage://toggle").unwrap(), UriAction::Toggle);
        assert_eq!(parse("vantage://show").unwrap(), UriAction::Show);
        assert_eq!(parse("vantage://status").unwrap(), UriAction::Show);
    }

    #[test]
    fn parses_select_with_node_param() {
        let a = parse("vantage://select?group=proxy&node=hk-01").unwrap();
        assert_eq!(
            a,
            UriAction::Select {
                group: "proxy".into(),
                name: "hk-01".into()
            }
        );
    }

    #[test]
    fn parses_select_with_name_param_and_percent_encoding() {
        let a = parse("vantage://select?group=proxy&name=Caf%C3%A9").unwrap();
        assert_eq!(
            a,
            UriAction::Select {
                group: "proxy".into(),
                name: "Café".into() // i18n-allow-non-english
            }
        );
    }

    #[test]
    fn rejects_empty_select_params() {
        assert!(parse("vantage://select?group=&node=").is_err());
        assert!(parse("vantage://select").is_err());
    }

    #[test]
    fn rejects_path_separators_even_when_encoded() {
        assert!(parse("vantage://select?group=a/b&node=x").is_err());
        assert!(parse("vantage://select?group=a%2Fb&node=x").is_err());
        assert!(parse("vantage://select?group=a&node=x%00").is_err());
    }

    #[test]
    fn rejects_unknown_and_non_vantage() {
        assert!(parse("vantage://frobnicate").is_err());
        assert!(parse("https://vantage-box/start").is_err());
        assert!(parse("install").is_err());
    }

    #[test]
    fn extract_uri_finds_token() {
        let args = vec![
            "C:\\app.exe".to_string(),
            FLAG.to_string(),
            "vantage://toggle".to_string(),
        ];
        assert_eq!(
            extract_uri(args.into_iter()),
            Some("vantage://toggle".into())
        );
    }

    #[test]
    fn extract_uri_none_when_absent() {
        let args = vec!["C:\\app.exe".to_string(), "--scm".to_string()];
        assert_eq!(extract_uri(args.into_iter()), None);
    }
}
