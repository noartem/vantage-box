//! Модели ответов Clash API в том виде, в котором их отдаёт sing-box.
//!
//! Все поля, которых может не быть в конкретной версии, помечены `default` —
//! ломаться из-за нового/пропавшего ключа приложение не должно.

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
    /// Текущий выбор — только у групп.
    #[serde(default)]
    pub now: Option<String>,
    /// Состав группы — только у групп.
    #[serde(default)]
    pub all: Option<Vec<String>>,
    #[serde(default)]
    pub history: Vec<DelayHistory>,
    #[serde(default)]
    pub udp: bool,
}

impl Proxy {
    /// Группа — это то, у чего есть список вложенных outbound'ов.
    pub fn is_group(&self) -> bool {
        self.all.as_ref().is_some_and(|all| !all.is_empty())
    }

    /// Только у `Selector` выбор можно менять руками; `URLTest` решает сам.
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

#[derive(Debug, Clone, Deserialize)]
pub struct RawLogEntry {
    #[serde(rename = "type", default)]
    pub level: String,
    #[serde(default)]
    pub payload: String,
}

/// Запись лога, обогащённая временем получения и монотонным id —
/// по нему UI дедуплицирует и виртуализирует ленту.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    /// Unix-время в миллисекундах.
    pub time: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelayResponse {
    #[serde(default)]
    pub delay: u32,
}

/// Состояние подключения к Clash API. Единственный источник правды для
/// индикатора в UI.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    /// Версия sing-box, если удалось её получить.
    pub version: Option<String>,
    /// Текст последней ошибки — показываем пользователю как есть.
    pub error: Option<String>,
    /// Попадает ли версия в поддерживаемый диапазон.
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
    /// Версию определить не удалось.
    Unknown,
    Supported,
    /// Версия ниже поддерживаемого диапазона.
    TooOld,
    /// Версия выше — работаем, но предупреждаем.
    TooNew,
}
