//! Фоновые подписки на Clash API: health-poller по HTTP и четыре WebSocket-потока
//! (`/traffic`, `/logs`, `/memory`, `/connections`). Всё, что приходит, уезжает
//! во фронтенд через Tauri-события — состояние в Rust не копим.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use futures_util::StreamExt;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::client::{compatibility, ClashClient};
use super::models::*;
use crate::settings::LogLevel;

pub const EVENT_STATUS: &str = "clash://status";
pub const EVENT_TRAFFIC: &str = "clash://traffic";
pub const EVENT_LOG: &str = "clash://log";
pub const EVENT_MEMORY: &str = "clash://memory";
pub const EVENT_CONNECTIONS: &str = "clash://connections";

/// Как часто опрашиваем `/version`, чтобы знать, жив ли sing-box.
const HEALTH_INTERVAL: Duration = Duration::from_secs(3);
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(15);

/// Владеет фоновыми задачами и текущим статусом подключения.
pub struct StreamManager {
    app: AppHandle,
    tasks: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
    status: RwLock<ConnectionStatus>,
    log_seq: Arc<AtomicU64>,
}

impl StreamManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            tasks: Mutex::new(Vec::new()),
            status: RwLock::new(ConnectionStatus::default()),
            log_seq: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn status(&self) -> ConnectionStatus {
        self.status.read().expect("status lock").clone()
    }

    /// Останавливает текущие подписки и поднимает новые под новый клиент.
    /// Вызывается на старте и на каждое изменение настроек Clash API.
    pub fn restart(self: &Arc<Self>, client: ClashClient, log_level: LogLevel) {
        self.stop();
        self.set_status(ConnectionStatus {
            state: ConnectionState::Connecting,
            ..ConnectionStatus::default()
        });

        let mut tasks = self.tasks.lock().expect("tasks lock");

        tasks.push(spawn_health(Arc::clone(self), client.clone()));

        tasks.push(spawn_ws(
            Arc::clone(self),
            client.ws_url("/traffic"),
            client.secret().to_string(),
            |app, _seq, text| {
                if let Ok(traffic) = serde_json::from_str::<Traffic>(text) {
                    let _ = app.emit(EVENT_TRAFFIC, traffic);
                }
            },
        ));

        tasks.push(spawn_ws(
            Arc::clone(self),
            format!("{}?level={}", client.ws_url("/logs"), log_level.as_str()),
            client.secret().to_string(),
            |app, seq, text| {
                if let Ok(raw) = serde_json::from_str::<RawLogEntry>(text) {
                    let entry = LogEntry {
                        id: seq.fetch_add(1, Ordering::Relaxed),
                        time: now_millis(),
                        level: if raw.level.is_empty() {
                            "info".into()
                        } else {
                            raw.level
                        },
                        message: raw.payload,
                    };
                    let _ = app.emit(EVENT_LOG, entry);
                }
            },
        ));

        tasks.push(spawn_ws(
            Arc::clone(self),
            client.ws_url("/memory"),
            client.secret().to_string(),
            |app, _seq, text| {
                if let Ok(memory) = serde_json::from_str::<Memory>(text) {
                    let _ = app.emit(EVENT_MEMORY, memory);
                }
            },
        ));

        // sing-box шлёт полный снимок соединений на каждое изменение —
        // последовательность не нужна, просто переизлучаем его в UI.
        tasks.push(spawn_ws(
            Arc::clone(self),
            client.ws_url("/connections"),
            client.secret().to_string(),
            |app, _seq, text| {
                if let Ok(snapshot) = serde_json::from_str::<ConnectionsSnapshot>(text) {
                    let _ = app.emit(EVENT_CONNECTIONS, snapshot);
                }
            },
        ));
    }

    /// Снимает все фоновые задачи. Идемпотентна.
    pub fn stop(&self) {
        let mut tasks = self.tasks.lock().expect("tasks lock");
        for task in tasks.drain(..) {
            task.abort();
        }
    }

    /// Обновляет статус и шлёт событие только при реальном изменении —
    /// иначе health-poller раз в три секунды дёргал бы UI впустую.
    fn set_status(&self, next: ConnectionStatus) {
        {
            let mut guard = self.status.write().expect("status lock");
            if *guard == next {
                return;
            }
            *guard = next.clone();
        }
        let _ = self.app.emit(EVENT_STATUS, next);
    }
}

fn spawn_health(
    manager: Arc<StreamManager>,
    client: ClashClient,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            match client.version().await {
                Ok(info) => manager.set_status(ConnectionStatus {
                    state: ConnectionState::Connected,
                    compatibility: compatibility(&info.version),
                    version: Some(info.version),
                    error: None,
                }),
                Err(err) => manager.set_status(ConnectionStatus {
                    state: ConnectionState::Disconnected,
                    version: None,
                    error: Some(err.to_string()),
                    compatibility: Compatibility::Unknown,
                }),
            }
            tokio::time::sleep(HEALTH_INTERVAL).await;
        }
    })
}

/// Держит один WebSocket живым: переподключается с экспоненциальной задержкой,
/// каждое текстовое сообщение отдаёт в `handler`.
fn spawn_ws<F>(
    manager: Arc<StreamManager>,
    url: String,
    secret: String,
    handler: F,
) -> tauri::async_runtime::JoinHandle<()>
where
    F: Fn(&AppHandle, &AtomicU64, &str) + Send + Sync + 'static,
{
    tauri::async_runtime::spawn(async move {
        let mut backoff = BACKOFF_START;
        loop {
            match connect(&url, &secret).await {
                Ok(mut ws) => {
                    backoff = BACKOFF_START;
                    while let Some(message) = ws.next().await {
                        match message {
                            Ok(Message::Text(text)) => {
                                handler(&manager.app, &manager.log_seq, text.as_str())
                            }
                            Ok(Message::Close(_)) | Err(_) => break,
                            // Ping/Pong обрабатывает сам tungstenite.
                            Ok(_) => {}
                        }
                    }
                }
                Err(_) => {
                    // Молчим: причину отказа уже показывает health-poller,
                    // дублировать её на каждый из трёх сокетов незачем.
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_MAX);
        }
    })
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn connect(url: &str, secret: &str) -> Result<WsStream, String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    if !secret.is_empty() {
        let value = format!("Bearer {secret}")
            .parse()
            .map_err(|_| "некорректный secret".to_string())?;
        request.headers_mut().insert("Authorization", value);
    }
    let (stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| e.to_string())?;
    Ok(stream)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
