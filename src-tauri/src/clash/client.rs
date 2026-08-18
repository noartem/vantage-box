//! HTTP client for the Clash API.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use super::models::*;
use crate::error::{Error, Result};
use crate::settings::ClashApiSettings;

/// The range of sing-box versions this release of Vantage Box is tested on.
/// Outside the range the app keeps working, but the UI shows a warning and
/// auto-update of the binary never goes beyond these bounds.
///
/// The bounds are not picked by eye: they come from `scripts/compat-matrix.ps1`,
/// which runs probes across every minor branch. The lower bound is the oldest
/// version we actually verified; earlier ones may work too, but we cannot
/// promise that.
///
/// Measured August 7, 2026: 1.10.7, 1.11.15, 1.12.25, 1.13.16 — all probes pass.
pub const SINGBOX_MIN: (u32, u32, u32) = (1, 10, 7);
pub const SINGBOX_MAX_EXCLUSIVE: (u32, u32, u32) = (1, 14, 0);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ClashClient {
    /// The base URL without a trailing slash, e.g. `http://127.0.0.1:9090`.
    base: String,
    secret: String,
    http: reqwest::Client,
}

impl ClashClient {
    pub fn new(settings: &ClashApiSettings) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // The Clash API lives on loopback: a system proxy only gets in the way here.
            .no_proxy()
            .build()?;

        Ok(Self {
            base: normalize_base_url(&settings.url),
            secret: settings.secret.clone(),
            http,
        })
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// `http://…` → `ws://…`, `https://…` → `wss://…`.
    pub fn ws_url(&self, path: &str) -> String {
        let base = if let Some(rest) = self.base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else if let Some(rest) = self.base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else {
            self.base.clone()
        };
        format!("{base}{path}")
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let builder = self.http.request(method, format!("{}{}", self.base, path));
        if self.secret.is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.secret)
        }
    }

    // -- endpoints ---------------------------------------------------------

    pub async fn version(&self) -> Result<VersionInfo> {
        self.send(self.request(reqwest::Method::GET, "/version")).await
    }

    pub async fn proxies(&self) -> Result<ProxiesResponse> {
        self.send(self.request(reqwest::Method::GET, "/proxies")).await
    }

    /// Switches a selector group to a specific outbound.
    pub async fn select(&self, group: &str, name: &str) -> Result<()> {
        let path = format!("/proxies/{}", urlencode(group));
        let req = self
            .request(reqwest::Method::PUT, &path)
            .json(&json!({ "name": name }));
        self.send_no_content(req).await
    }

    /// Measures the delay of a single outbound, ms.
    pub async fn proxy_delay(&self, name: &str, url: &str, timeout: u32) -> Result<u32> {
        let path = format!("/proxies/{}/delay", urlencode(name));
        let req = self
            .request(reqwest::Method::GET, &path)
            .query(&[("url", url), ("timeout", &timeout.to_string())]);
        let resp: DelayResponse = self.send(req).await?;
        Ok(resp.delay)
    }

    /// Measures the delay of a whole group at once: `{ "outbound": delay_ms }`.
    /// Unreachable nodes sing-box simply omits from the response.
    pub async fn group_delay(
        &self,
        group: &str,
        url: &str,
        timeout: u32,
    ) -> Result<HashMap<String, u32>> {
        let path = format!("/group/{}/delay", urlencode(group));
        let req = self
            .request(reqwest::Method::GET, &path)
            .query(&[("url", url), ("timeout", &timeout.to_string())]);
        self.send(req).await
    }

    /// The current sing-box runtime config (what is actually applied).
    pub async fn configs(&self) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::GET, "/configs")).await
    }

    /// A snapshot of active connections. Duplicates the WebSocket `/connections`,
    /// but is needed for a one-off request (for example, when opening a tab).
    pub async fn connections(&self) -> Result<ConnectionsSnapshot> {
        self.send(self.request(reqwest::Method::GET, "/connections")).await
    }

    /// Closes one connection. sing-box ids can contain characters that need
    /// percent-encoding.
    pub async fn close_connection(&self, id: &str) -> Result<()> {
        let path = format!("/connections/{}", urlencode(id));
        self.send_no_content(self.request(reqwest::Method::DELETE, &path))
            .await
    }

    /// Closes all connections.
    pub async fn close_all_connections(&self) -> Result<()> {
        self.send_no_content(self.request(reqwest::Method::DELETE, "/connections"))
            .await
    }

    // -- transport ---------------------------------------------------------

    async fn send<T: serde::de::DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T> {
        let resp = self.check(req).await?;
        let body = resp.text().await?;
        serde_json::from_str(&body).map_err(|e| Error::parse("Clash API response", e))
    }

    async fn send_no_content(&self, req: reqwest::RequestBuilder) -> Result<()> {
        self.check(req).await?;
        Ok(())
    }

    async fn check(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let resp = req.send().await.map_err(|e| {
            // 401 is caught below; only network problems reach here.
            Error::Transport(friendly_transport_error(&e))
        })?;

        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let body = resp.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| {
                if body.trim().is_empty() {
                    status.canonical_reason().unwrap_or("error").to_string()
                } else {
                    body.trim().to_string()
                }
            });

        let message = if status == reqwest::StatusCode::UNAUTHORIZED {
            format!("{message} — check the secret in settings")
        } else {
            message
        };

        Err(Error::Api {
            status: status.as_u16(),
            message,
        })
    }
}

fn friendly_transport_error(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "could not connect — sing-box is not running or the Clash API listens on a different address".into()
    } else if e.is_timeout() {
        "request timed out".into()
    } else {
        e.to_string()
    }
}

/// Brings user input to `scheme://host:port` without a trailing slash.
/// An empty string and a bare `host:port` are common cases — we fix them silently.
pub fn normalize_base_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "http://127.0.0.1:9090".into();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

/// Percent-encoding for group names: sing-box tags can have spaces and unicode.
pub(crate) fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Strips the prefix that sing-box writes itself from a bare version:
/// the `version` field of the `/version` response is `"sing-box 1.13.18"`, the
/// first line of `sing-box version` output is `"sing-box version 1.11.4"`.
/// Without this the status line shows "sing-box sing-box …", and `parse_version`
/// trips on the hyphen in "sing-box". Case and a leading `v` are also stripped.
pub fn normalize_version(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    // The cut prefix length is the same in `lower` and the source string, so
    // we slice the source by it — preserving the case of the suffix (v1.x, beta…).
    let prefix_len = if lower.starts_with("sing-box version ") {
        "sing-box version ".len()
    } else if lower.starts_with("sing-box ") {
        "sing-box ".len()
    } else {
        0
    };
    trimmed[prefix_len..]
        .trim()
        .trim_start_matches('v')
        .to_string()
}

/// Parses `1.12.0-beta.3` into `(1, 12, 0)`; suffixes are ignored.
pub fn parse_version(raw: &str) -> Option<(u32, u32, u32)> {
    let core = normalize_version(raw);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn compatibility(raw_version: &str) -> Compatibility {
    match parse_version(raw_version) {
        None => Compatibility::Unknown,
        Some(v) if v < SINGBOX_MIN => Compatibility::TooOld,
        Some(v) if v >= SINGBOX_MAX_EXCLUSIVE => Compatibility::TooNew,
        Some(_) => Compatibility::Supported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize_base_url(""), "http://127.0.0.1:9090");
        assert_eq!(normalize_base_url("127.0.0.1:9090"), "http://127.0.0.1:9090");
        assert_eq!(
            normalize_base_url(" http://127.0.0.1:9090/ "),
            "http://127.0.0.1:9090"
        );
        assert_eq!(normalize_base_url("https://localhost:9"), "https://localhost:9");
    }

    #[test]
    fn builds_ws_urls() {
        let client = ClashClient::new(&ClashApiSettings::default()).unwrap();
        let expected = format!(
            "{}/traffic",
            crate::settings::DEFAULT_CLASH_URL.replace("http://", "ws://")
        );
        assert_eq!(client.ws_url("/traffic"), expected);
    }

    #[test]
    fn encodes_group_tags() {
        assert_eq!(urlencode("my group"), "my%20group");
        assert_eq!(urlencode("proxy-1_a.b~c"), "proxy-1_a.b~c");
    }

    #[test]
    fn normalizes_versions() {
        // `/version` returns the version with a "sing-box " prefix.
        assert_eq!(normalize_version("sing-box 1.13.18"), "1.13.18");
        // `sing-box version` writes "sing-box version …".
        assert_eq!(normalize_version("sing-box version 1.11.4"), "1.11.4");
        // A bare version and a leading `v` stay compatible.
        assert_eq!(normalize_version("1.13.18"), "1.13.18");
        assert_eq!(normalize_version("v1.12.0-beta.3"), "1.12.0-beta.3");
        assert_eq!(normalize_version("  sing-box 1.13.18  "), "1.13.18");
        assert_eq!(normalize_version(""), "");
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("1.12.0"), Some((1, 12, 0)));
        assert_eq!(parse_version("v1.12.0-beta.3"), Some((1, 12, 0)));
        assert_eq!(parse_version("1.11"), Some((1, 11, 0)));
        // sing-box returns the version with a prefix — the parser must handle it.
        assert_eq!(parse_version("sing-box 1.13.18"), Some((1, 13, 18)));
        assert_eq!(parse_version("sing-box version 1.11.4"), Some((1, 11, 4)));
        assert_eq!(parse_version("не-версия"), None); // i18n-allow-non-english
    }

    #[test]
    fn classifies_compatibility() {
        assert_eq!(compatibility("1.13.16"), Compatibility::Supported);
        assert_eq!(compatibility("1.10.7"), Compatibility::Supported);
        assert_eq!(compatibility("1.10.6"), Compatibility::TooOld);
        assert_eq!(compatibility("1.14.0"), Compatibility::TooNew);
        // The prefix from the `/version` response must not break classification.
        assert_eq!(compatibility("sing-box 1.13.18"), Compatibility::Supported);
        assert_eq!(compatibility("sing-box 1.14.0"), Compatibility::TooNew);
        assert_eq!(compatibility(""), Compatibility::Unknown);
    }
}