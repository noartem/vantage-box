//! HTTP-клиент Clash API.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use super::models::*;
use crate::error::{Error, Result};
use crate::settings::ClashApiSettings;

/// Диапазон версий sing-box, на котором этот релиз Vantage Box протестирован.
/// Вне диапазона приложение продолжает работать, но UI показывает предупреждение,
/// а автообновление бинарника никогда не выходит за эти границы.
///
/// Границы не назначаются на глаз: их даёт `scripts/compat-matrix.ps1`, который
/// прогоняет пробы по каждой минорной ветке. Нижняя граница — самая старая
/// версия, которую мы действительно проверяли; более ранние, возможно, тоже
/// работают, но обещать это не за что.
///
/// Измерено 7 августа 2026: 1.10.7, 1.11.15, 1.12.25, 1.13.16 — все пробы прошли.
pub const SINGBOX_MIN: (u32, u32, u32) = (1, 10, 7);
pub const SINGBOX_MAX_EXCLUSIVE: (u32, u32, u32) = (1, 14, 0);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ClashClient {
    /// Базовый URL без завершающего слэша, например `http://127.0.0.1:9090`.
    base: String,
    secret: String,
    http: reqwest::Client,
}

impl ClashClient {
    pub fn new(settings: &ClashApiSettings) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // Clash API живёт на loopback: системный прокси тут только мешает.
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

    // -- эндпоинты ---------------------------------------------------------

    pub async fn version(&self) -> Result<VersionInfo> {
        self.send(self.request(reqwest::Method::GET, "/version")).await
    }

    pub async fn proxies(&self) -> Result<ProxiesResponse> {
        self.send(self.request(reqwest::Method::GET, "/proxies")).await
    }

    /// Переключает selector-группу на конкретный outbound.
    pub async fn select(&self, group: &str, name: &str) -> Result<()> {
        let path = format!("/proxies/{}", urlencode(group));
        let req = self
            .request(reqwest::Method::PUT, &path)
            .json(&json!({ "name": name }));
        self.send_no_content(req).await
    }

    /// Замер задержки одного outbound'а, мс.
    pub async fn proxy_delay(&self, name: &str, url: &str, timeout: u32) -> Result<u32> {
        let path = format!("/proxies/{}/delay", urlencode(name));
        let req = self
            .request(reqwest::Method::GET, &path)
            .query(&[("url", url), ("timeout", &timeout.to_string())]);
        let resp: DelayResponse = self.send(req).await?;
        Ok(resp.delay)
    }

    /// Замер задержки всей группы разом: `{ "outbound": delay_ms }`.
    /// Недоступные узлы sing-box просто не включает в ответ.
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

    /// Текущий рантайм-конфиг sing-box (то, что реально применено).
    pub async fn configs(&self) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::GET, "/configs")).await
    }

    /// Снимок активных соединений. Дублирует WebSocket `/connections`, но
    /// нужен для разового запроса (например, при открытии вкладки).
    pub async fn connections(&self) -> Result<ConnectionsSnapshot> {
        self.send(self.request(reqwest::Method::GET, "/connections")).await
    }

    /// Закрывает одно соединение. Идентификаторы sing-box бывают с символами,
    /// требующими percent-encoding.
    pub async fn close_connection(&self, id: &str) -> Result<()> {
        let path = format!("/connections/{}", urlencode(id));
        self.send_no_content(self.request(reqwest::Method::DELETE, &path))
            .await
    }

    /// Закрывает все соединения.
    pub async fn close_all_connections(&self) -> Result<()> {
        self.send_no_content(self.request(reqwest::Method::DELETE, "/connections"))
            .await
    }

    // -- транспорт ---------------------------------------------------------

    async fn send<T: serde::de::DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T> {
        let resp = self.check(req).await?;
        let body = resp.text().await?;
        serde_json::from_str(&body).map_err(|e| Error::parse("ответ Clash API", e))
    }

    async fn send_no_content(&self, req: reqwest::RequestBuilder) -> Result<()> {
        self.check(req).await?;
        Ok(())
    }

    async fn check(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let resp = req.send().await.map_err(|e| {
            // Отдельно ловим 401 ниже; сюда попадают только сетевые проблемы.
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
                    status.canonical_reason().unwrap_or("ошибка").to_string()
                } else {
                    body.trim().to_string()
                }
            });

        let message = if status == reqwest::StatusCode::UNAUTHORIZED {
            format!("{message} — проверьте secret в настройках")
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
        "не удалось подключиться — sing-box не запущен или Clash API слушает другой адрес".into()
    } else if e.is_timeout() {
        "истёк таймаут запроса".into()
    } else {
        e.to_string()
    }
}

/// Приводит пользовательский ввод к `scheme://host:port` без хвостового слэша.
/// Пустая строка и голый `host:port` — частые случаи, их чиним молча.
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

/// Percent-encoding для имён групп: теги в sing-box бывают с пробелами и юникодом.
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

/// Разбирает `1.12.0-beta.3` в `(1, 12, 0)`; суффиксы игнорируем.
pub fn parse_version(raw: &str) -> Option<(u32, u32, u32)> {
    let core = raw.trim().trim_start_matches('v');
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
    fn parses_versions() {
        assert_eq!(parse_version("1.12.0"), Some((1, 12, 0)));
        assert_eq!(parse_version("v1.12.0-beta.3"), Some((1, 12, 0)));
        assert_eq!(parse_version("1.11"), Some((1, 11, 0)));
        assert_eq!(parse_version("не-версия"), None);
    }

    #[test]
    fn classifies_compatibility() {
        assert_eq!(compatibility("1.13.16"), Compatibility::Supported);
        assert_eq!(compatibility("1.10.7"), Compatibility::Supported);
        assert_eq!(compatibility("1.10.6"), Compatibility::TooOld);
        assert_eq!(compatibility("1.14.0"), Compatibility::TooNew);
        assert_eq!(compatibility(""), Compatibility::Unknown);
    }
}
