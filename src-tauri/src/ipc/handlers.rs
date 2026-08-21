//! JSON-RPC method handlers — a sibling of `commands.rs`.
//!
//! Both are thin wrappers over the same `actions::*`, `ClashClient` and
//! `window::*` functions, so the GUI and external integrations behave
//! identically. The bus is just one more client of the same operations.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::actions::{self, RunStatus};
use crate::clash::build_overview;
use crate::clash::models::ConnectionStatus;
use crate::error::Error;
use crate::runtime;
use crate::state::AppState;
use crate::subscription;
use crate::window;

use super::jsonrpc::{self, BUS_UNAVAILABLE, INTERNAL_ERROR, NOT_APPLICABLE, UNAUTHORIZED};
use super::jsonrpc::{Request, Response, RpcError};

/// Combined runtime view: how sing-box is run plus whether the Clash API is up.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub run: RunStatus,
    pub connection: ConnectionStatus,
}

// -- Typed params for the methods that take any ---------------------------------

#[derive(Debug, Deserialize)]
pub struct SelectParams {
    pub group: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct NameParams {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupParams {
    pub group: String,
}

#[derive(Debug, Deserialize)]
pub struct ForceParams {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct IdParams {
    pub id: String,
}

/// Dispatches one request to its handler. Notifications (no id) are ignored —
/// the CLI never sends them and the server has no use for client→server ones.
pub async fn dispatch(handle: &AppHandle, req: Request) -> Response {
    if req.is_notification() {
        // No response is expected for a notification.
        return Response::success(req.echo_id(), Value::Null);
    }

    let id = req.echo_id();
    let method = req.method.as_str();

    let res: std::result::Result<Value, RpcError> = match method {
        // -- lifecycle ---------------------------------------------------------
        "start" => actions::start(handle)
            .await
            .map_err(map_error)
            .and_then(serialize_value),
        "stop" => actions::stop(handle)
            .await
            .map_err(map_error)
            .and_then(serialize_value),
        "toggle" => actions::toggle(handle)
            .await
            .map_err(map_error)
            .and_then(serialize_value),
        "restart" => actions::restart(handle)
            .await
            .map_err(map_error)
            .and_then(serialize_value),
        "installService" => actions::install(handle)
            .await
            .map_err(map_error)
            .and_then(serialize_value),
        "uninstallService" => actions::uninstall(handle)
            .await
            .map_err(map_error)
            .and_then(serialize_value),

        // -- status ------------------------------------------------------------
        "status" => {
            let connection = handle.state::<AppState>().streams.status();
            match actions::run_status(handle).await {
                Ok(run) => serialize_value(StatusReport { run, connection }),
                Err(e) => Err(map_error(e)),
            }
        }
        "runtimeConfig" => runtime::read_runtime_config()
            .map_err(map_error)
            .and_then(serialize_value),

        // -- proxies -----------------------------------------------------------
        "proxies" => {
            let state = handle.state::<AppState>();
            state
                .client()
                .proxies()
                .await
                .map_err(map_error)
                .and_then(|resp| serialize_value(build_overview(resp.proxies)))
        }
        "select" => {
            let p = match req.params_as::<SelectParams>() {
                Ok(p) => p,
                Err(e) => return Response::error(id, e),
            };
            actions::select_proxy(handle, &p.group, &p.name)
                .await
                .map_err(map_error)
                .map(|_| Value::Null)
        }
        "testDelay" => {
            let p = match req.params_as::<NameParams>() {
                Ok(p) => p,
                Err(e) => return Response::error(id, e),
            };
            let state = handle.state::<AppState>();
            let settings = state.settings.get();
            state
                .client()
                .proxy_delay(
                    &p.name,
                    &settings.ui.latency_test_url,
                    settings.ui.latency_test_timeout,
                )
                .await
                .map_err(map_error)
                .and_then(serialize_value)
        }
        "testGroupDelay" => {
            let p = match req.params_as::<GroupParams>() {
                Ok(p) => p,
                Err(e) => return Response::error(id, e),
            };
            let state = handle.state::<AppState>();
            let settings = state.settings.get();
            state
                .client()
                .group_delay(
                    &p.group,
                    &settings.ui.latency_test_url,
                    settings.ui.latency_test_timeout,
                )
                .await
                .map_err(map_error)
                .and_then(serialize_value)
        }

        // -- connections -------------------------------------------------------
        "connections" => handle
            .state::<AppState>()
            .client()
            .connections()
            .await
            .map_err(map_error)
            .and_then(serialize_value),
        "closeConnection" => {
            let p = match req.params_as::<IdParams>() {
                Ok(p) => p,
                Err(e) => return Response::error(id, e),
            };
            handle
                .state::<AppState>()
                .client()
                .close_connection(&p.id)
                .await
                .map_err(map_error)
                .map(|_| Value::Null)
        }
        "closeAllConnections" => handle
            .state::<AppState>()
            .client()
            .close_all_connections()
            .await
            .map_err(map_error)
            .map(|_| Value::Null),

        // -- subscriptions -----------------------------------------------------
        "subscriptions.refresh" => {
            let p = match req.params_as::<ForceParams>() {
                Ok(p) => p,
                Err(e) => return Response::error(id, e),
            };
            subscription::apply(handle, p.force)
                .await
                .map_err(map_error)
                .and_then(serialize_value)
        }
        "subscriptions.state" => subscription::load_state()
            .map_err(map_error)
            .and_then(serialize_value),

        // -- UI ----------------------------------------------------------------
        "ui.showMain" => {
            window::show_main(handle);
            Ok(Value::Null)
        }
        "ui.togglePopup" => {
            window::toggle_popup(handle);
            Ok(Value::Null)
        }
        "ui.closePopup" => {
            window::hide_popup(handle);
            Ok(Value::Null)
        }

        _ => return jsonrpc::method_not_found(id, method),
    };

    match res {
        Ok(value) => Response::success(id, value),
        Err(err) => Response::error(id, err),
    }
}

/// Maps an `error::Error` to the JSON-RPC error code that best describes it for
/// a machine client (the CLI relies on `BUS_UNAVAILABLE` and `UNAUTHORIZED` to
/// pick its exit code).
fn map_error(e: Error) -> RpcError {
    match e {
        Error::Transport(_) => RpcError::new(BUS_UNAVAILABLE, e.to_string()),
        Error::Api {
            status: 401,
            ref message,
        } => RpcError::new(UNAUTHORIZED, message.clone()),
        // "the config needs TUN …" / "sing-box is running as a service — stop it
        // before …": the action is fine, it just does not apply right now.
        Error::Other(ref msg) if msg.contains("needs TUN") || msg.contains("stop it before") => {
            RpcError::new(NOT_APPLICABLE, msg.clone())
        }
        other => RpcError::new(INTERNAL_ERROR, other.to_string()),
    }
}

fn serialize_value<T: Serialize>(v: T) -> std::result::Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))
}
