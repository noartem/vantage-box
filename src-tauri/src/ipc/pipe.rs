//! The named-pipe server: `\\.\pipe\vantage-box\control`, JSON-RPC 2.0 line
//! protocol, one tokio task per client.
//!
//! The pipe lives only while the Tauri app runs — it is the broker. When the
//! app quits, the listener goes away and CLI clients fail to connect (exit
//! code 3). That is the documented limitation: for a tunnel that survives the
//! app, use service mode.

use std::os::windows::io::AsRawHandle;
use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use tokio::sync::Mutex;
use windows_sys::Win32::Foundation::HANDLE;

use super::acl::PipeAcl;
use super::handlers;
use super::jsonrpc::{Notification, Request, Response, RpcError, PARSE_ERROR};
use super::BusSubscribers;

/// Fixed pipe path — clients (the CLI, future PowerShell/MCP) know it.
pub const PIPE_NAME: &str = r"\\.\pipe\vantage-box\control";

/// Accept loop. Runs on the tauri async runtime; one task per client.
pub async fn serve(handle: AppHandle) {
    let acl = match PipeAcl::build() {
        Ok(acl) => acl,
        Err(e) => {
            eprintln!("vantage-box ipc: pipe ACL not built, bus disabled: {e}");
            return;
        }
    };

    loop {
        // A fresh pipe instance per client — the standard tokio named-pipe
        // server pattern. tokio has no security-attributes builder, so we
        // create with the default descriptor and set our DACL via
        // SetSecurityInfo before accepting a client (see `acl::apply_to`).
        let server = match ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .access_inbound(true)
            .access_outbound(true)
            // Lets the subsequent SetSecurityInfo(DACL) call succeed.
            .write_dac(true)
            // Defense-in-depth: even with the user-only ACL, refuse network
            // (SMB) clients at the pipe layer.
            .reject_remote_clients(true)
            .create(PIPE_NAME)
        {
            Ok(server) => server,
            Err(e) => {
                // Another instance already holds the name, or the ACL is bad —
                // either way the bus cannot start. Do not spin: log and stop.
                eprintln!("vantage-box ipc: could not create the control pipe (bus disabled): {e}");
                return;
            }
        };

        // Tighten the instance to current-user + LocalSystem before a client
        // can connect. A failure here is not fatal to the whole bus: the
        // default descriptor already excludes remote clients (above) and
        // non-admins, so we log and keep going on the default DACL.
        if let Err(e) = unsafe { acl.apply_to(server.as_raw_handle() as HANDLE) } {
            eprintln!("vantage-box ipc: could not set the pipe DACL ({e}); continuing on the default DACL");
        }

        if let Err(e) = server.connect().await {
            eprintln!("vantage-box ipc: pipe connect failed: {e}");
            continue;
        }

        let h = handle.clone();
        tauri::async_runtime::spawn(async move {
            handle_client(h, server).await;
        });
    }
}

/// Reads line-delimited JSON-RPC requests and writes responses; concurrently
/// forwards bus notifications (`state_changed`, `proxies_changed`) to the client.
async fn handle_client(handle: AppHandle, stream: NamedPipeServer) {
    let Some(bus) = handle.try_state::<BusSubscribers>() else {
        return;
    };
    let mut subs = bus.subscribe();

    let (read, write) = tokio::io::split(stream);
    let write = Arc::new(Mutex::new(write));
    let mut reader = BufReader::new(read);
    let mut buf = String::new();

    loop {
        tokio::select! {
            // A request line from the client.
            n = reader.read_line(&mut buf) => {
                match n {
                    Ok(0) => break, // EOF — client gone
                    Ok(_) => {
                        let line = buf.trim_end();
                        if !line.is_empty() {
                            let resp = match serde_json::from_str::<Request>(line) {
                                Ok(req) => handlers::dispatch(&handle, req).await,
                                Err(e) => Response::error(
                                    Value::Null,
                                    RpcError::new(PARSE_ERROR, format!("parse error: {e}")),
                                ),
                            };
                            let mut w = write.lock().await;
                            if w.write_all(resp.to_line().as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        buf.clear();
                    }
                    Err(_) => break,
                }
            }
            // A server→client notification from the bus.
            recv = subs.recv() => {
                match recv {
                    Ok(notif) => {
                        let mut w = write.lock().await;
                        if write_notification(&mut w, &notif).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // The client was slow; it just misses some notifications.
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // The bus is gone (app shutting down). Keep answering
                        // requests until the client disconnects, but no more
                        // notifications will arrive.
                    }
                }
            }
        }
    }
}

async fn write_notification(
    write: &mut tokio::sync::MutexGuard<'_, tokio::io::WriteHalf<NamedPipeServer>>,
    notif: &Notification,
) -> std::io::Result<()> {
    write.write_all(notif.to_line().as_bytes()).await
}
