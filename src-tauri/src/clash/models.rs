//! Models for the Clash API responses, in the shape sing-box returns them.
//!
//! All fields that may be absent in a particular version are marked `default`
//! — the app must not break on a new or removed key.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub premium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxiesResponse {
    pub proxies: HashMap<String, Proxy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    /// `Selector`, `URLTest`, `Direct`, `Shadowsocks`, …
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    /// The current selection — only on groups.
    #[serde(default)]
    pub now: Option<String>,
    /// The group members — only on groups.
    #[serde(default)]
    pub all: Option<Vec<String>>,
    #[serde(default)]
    pub history: Vec<DelayHistory>,
    #[serde(default)]
    pub udp: bool,
}

impl Proxy {
    /// A group is anything that has a list of nested outbounds.
    pub fn is_group(&self) -> bool {
        self.all.as_ref().is_some_and(|all| !all.is_empty())
    }

    /// Only on `Selector` can the selection be changed by hand; `URLTest`
    /// decides on its own.
    pub fn is_selectable(&self) -> bool {
        self.kind.eq_ignore_ascii_case("selector")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelayHistory {
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub delay: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Traffic {
    #[serde(default)]
    pub up: u64,
    #[serde(default)]
    pub down: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Memory {
    #[serde(default)]
    pub inuse: u64,
    #[serde(default)]
    pub oslimit: u64,
}

/// A snapshot of active connections from `/connections`. sing-box sends a full
/// snapshot on every change, so we do not need to accumulate anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsSnapshot {
    #[serde(default)]
    pub download_total: u64,
    #[serde(default)]
    pub upload_total: u64,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

/// One connection. sing-box has many fields and they change between versions,
/// so everything except the identifier and the volumes is `default`.
///
/// sing-box nests network addresses in a `metadata` sub-object (see
/// `experimental/clashapi/connections.go`) rather than flat — so the model
/// mirrors exactly that shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    #[serde(default)]
    pub id: String,
    /// The outbound chain: `[node, group]` from outer to inner.
    #[serde(default)]
    pub chains: Vec<String>,
    #[serde(default)]
    pub rule: String,
    #[serde(default, rename = "rulePayload")]
    pub rule_payload: String,
    #[serde(default)]
    pub metadata: ConnectionMetadata,
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    /// The start time, ISO format.
    #[serde(default)]
    pub start: String,
}

/// Network attributes of a connection — the `metadata` sub-object in the
/// sing-box response.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionMetadata {
    #[serde(default)]
    pub network: String,
    /// The inbound type: `Mixed`, `Tun`, …
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default, rename = "sourceIP")]
    pub source_ip: String,
    #[serde(default, rename = "sourcePort")]
    pub source_port: String,
    #[serde(default, rename = "destinationIP")]
    pub destination_ip: String,
    #[serde(default, rename = "destinationPort")]
    pub destination_port: String,
    #[serde(default)]
    pub host: String,
    #[serde(default, rename = "processPath")]
    pub process_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawLogEntry {
    #[serde(rename = "type", default)]
    pub level: String,
    #[serde(default)]
    pub payload: String,
}

/// A log entry enriched with the receive time and a monotonic id — the UI uses
/// it to deduplicate and virtualize the feed.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    /// Unix time in milliseconds.
    pub time: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelayResponse {
    #[serde(default)]
    pub delay: u32,
}

/// The connection state to the Clash API. The single source of truth for the
/// UI indicator.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    /// The sing-box version, if we managed to get it.
    pub version: Option<String>,
    /// The last error text — shown to the user as-is.
    pub error: Option<String>,
    /// Whether the version falls in the supported range.
    pub compatibility: Compatibility,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            version: None,
            error: None,
            compatibility: Compatibility::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Compatibility {
    /// The version could not be determined.
    Unknown,
    Supported,
    /// The version is below the supported range.
    TooOld,
    /// The version is above — we keep working, but warn.
    TooNew,
}
