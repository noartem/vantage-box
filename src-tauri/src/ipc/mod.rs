//! The local control bus: a Windows named pipe hosting JSON-RPC 2.0, started
//! inside the running Tauri app.
//!
//! The CLI and the URI handler are clients of this bus; the handlers reuse
//! `actions::*` / `ClashClient` / `window::*`, so external integrations behave
//! exactly like the GUI. The pipe lives only while the app runs — see
//! [`pipe`] for the headless limitation.
//!
//! On non-Windows the bus is a no-op stub for now (M3 will add a unix-socket
//! transport under the same protocol).

mod handlers;
pub mod jsonrpc;

#[cfg(windows)]
mod acl;
#[cfg(windows)]
mod pipe;

#[cfg(windows)]
pub use pipe::PIPE_NAME;

#[cfg(windows)]
use serde_json::Value;
#[cfg(windows)]
use tauri::{AppHandle, Listener, Manager};
#[cfg(windows)]
use tokio::sync::broadcast;

#[cfg(windows)]
use crate::actions::{EVENT_PROXIES, EVENT_SERVICE};

#[cfg(windows)]
use self::jsonrpc::Notification;

/// Subscribers to server→client notifications. One broadcast channel for the
/// whole app; each connected pipe client holds a receiver.
#[cfg(windows)]
pub struct BusSubscribers {
    tx: broadcast::Sender<Notification>,
}

#[cfg(windows)]
impl BusSubscribers {
    pub fn new() -> Self {
        // 64 is plenty: `state_changed`/`proxies_changed` fire a few times per
        // second at most, and slow clients just miss events (lagged receivers).
        let (tx, _rx) = broadcast::channel(64);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.tx.subscribe()
    }

    fn broadcast(&self, n: Notification) {
        let _ = self.tx.send(n);
    }
}

/// Starts the pipe server and the Tauri-event→notification forwarders. Call
/// once from `setup()` after `AppState` is managed. Returns immediately; the
/// bus runs for the app lifetime on the async runtime.
#[cfg(windows)]
pub fn start_server(handle: AppHandle) {
    handle.manage(BusSubscribers::new());

    // `service://changed` carries a `RunStatus` payload — forward it as a
    // `state_changed` notification so long-lived clients (future MCP) can react
    // without polling.
    let h = handle.clone();
    handle.listen(EVENT_SERVICE, move |event| {
        let params = serde_json::from_str::<Value>(event.payload()).unwrap_or(Value::Null);
        if let Some(bus) = h.try_state::<BusSubscribers>() {
            bus.broadcast(Notification::new("state_changed", params));
        }
    });

    // `proxies://changed` carries no payload — a selection changed, clients
    // should re-fetch `/proxies`.
    let h = handle.clone();
    handle.listen(EVENT_PROXIES, move |_event| {
        if let Some(bus) = h.try_state::<BusSubscribers>() {
            bus.broadcast(Notification::new("proxies_changed", Value::Null));
        }
    });

    let h = handle.clone();
    tauri::async_runtime::spawn(async move {
        pipe::serve(h).await;
    });
}

#[cfg(not(windows))]
pub fn start_server(_handle: AppHandle) {}
