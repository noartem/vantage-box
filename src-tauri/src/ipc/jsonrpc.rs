//! JSON-RPC 2.0 line protocol for the local control bus.
//!
//! One JSON object per line, separated by `\n`. The CLI and URI handler send
//! requests with ids; the server replies with responses and may push
//! notifications (no id) to subscribed clients for `state_changed` /
//! `proxies_changed`. Dependency-free: only `serde_json`.
//!
//! We are lenient about the `jsonrpc` version on input (we do not reject
//! requests that omit it) but always emit `"2.0"` on output — the contract for
//! clients is what we produce, not what we accept.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol marker written on every outgoing message.
pub const VERSION: &str = "2.0";

// -- JSON-RPC standard error codes ------------------------------------------

pub const PARSE_ERROR: i32 = -32700;
/// Standard JSON-RPC code for a structurally invalid request. Defined for
/// completeness of the contract; we currently emit [`PARSE_ERROR`] for bad
/// JSON instead, since `Request` deserializes almost anything via `#[serde(default)]`.
#[allow(dead_code)]
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

// -- App-specific error codes (the -32000 band is reserved for server use) ---

/// sing-box is not running or the Clash API is unreachable.
pub const BUS_UNAVAILABLE: i32 = -32000;
/// The Clash API rejected the bearer secret (HTTP 401).
pub const UNAUTHORIZED: i32 = -32001;
/// The action does not apply in the current mode (e.g. install while running).
pub const NOT_APPLICABLE: i32 = -32002;
/// An operation did not complete within the deadline. Part of the documented
/// contract; the `--wait` path currently maps to CLI exit 5 instead of an RPC
/// error, so the server does not emit this yet.
#[allow(dead_code)]
pub const TIMEOUT: i32 = -32003;
/// The user dismissed the URI confirmation dialog. Part of the documented
/// contract; the URI surface is fire-and-forget (no response channel), so this
/// is reserved rather than emitted.
#[allow(dead_code)]
pub const URI_CANCELLED: i32 = -32004;

/// A request, or a client→server notification when `id` is absent/null.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// A notification carries no id (or an explicit `null`): the server must
    /// not reply, and the CLI never sends one.
    pub fn is_notification(&self) -> bool {
        matches!(&self.id, None | Some(Value::Null))
    }

    /// The id to echo back in a response, or `Null` for notifications (which
    /// never get a response anyway).
    pub fn echo_id(&self) -> Value {
        self.id.clone().unwrap_or(Value::Null)
    }

    /// Parse `params` into a typed struct for a handler.
    pub fn params_as<T: for<'de> Deserialize<'de>>(&self) -> Result<T, RpcError> {
        serde_json::from_value::<T>(self.params.clone())
            .map_err(|e| RpcError::new(INVALID_PARAMS, format!("invalid params: {e}")))
    }
}

/// A response: exactly one of `result` / `error` is set.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, err: RpcError) -> Self {
        Self {
            jsonrpc: VERSION,
            id,
            result: None,
            error: Some(err),
        }
    }

    /// One line ready to write to the pipe: the JSON object followed by `\n`.
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_else(|_| "{}".into());
        line.push('\n');
        line
    }
}

/// A server→client notification (no id, no response).
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: Value,
}

impl Notification {
    pub fn new(method: &'static str, params: Value) -> Self {
        Self {
            jsonrpc: VERSION,
            method,
            params,
        }
    }

    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_else(|_| "{}".into());
        line.push('\n');
        line
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Attach structured `data` to an error. Reserved for handlers that want
    /// to return machine-readable detail alongside the message; none do yet.
    #[allow(dead_code)]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Convenience constructor for the method-not-found response. Handlers build
/// other errors via `map_error` / `serialize_value` (which return `RpcError`,
/// wrapped into a `Response::error` at the end of `dispatch`).
pub fn method_not_found(id: Value, method: &str) -> Response {
    Response::error(
        id,
        RpcError::new(METHOD_NOT_FOUND, format!("method not found: {method}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_request_with_params() {
        let raw = r#"{"jsonrpc":"2.0","id":7,"method":"select","params":{"group":"proxy","name":"node-1"}}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "select");
        assert!(!req.is_notification());
        assert_eq!(req.echo_id(), json!(7));

        #[derive(Deserialize)]
        struct P {
            group: String,
            name: String,
        }
        let p: P = req.params_as().unwrap();
        assert_eq!(p.group, "proxy");
        assert_eq!(p.name, "node-1");
    }

    #[test]
    fn treats_null_id_as_notification() {
        let raw = r#"{"jsonrpc":"2.0","method":"state_changed","params":null}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert!(req.is_notification());
        assert_eq!(req.echo_id(), Value::Null);
    }

    #[test]
    fn bad_params_yield_invalid_params_error() {
        let req: Request = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":1,"method":"select","params":"not an object"}"#,
        )
        .unwrap();
        let err: Result<(String,), RpcError> = req.params_as();
        let err = err.unwrap_err();
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn success_response_serializes_without_error_field() {
        let resp = Response::success(json!(1), json!({"running": true}));
        let line = resp.to_line();
        assert!(line.ends_with('\n'));
        assert!(line.contains("\"result\""));
        assert!(!line.contains("\"error\""));
        assert!(line.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn error_response_carries_code_and_message() {
        let resp = Response::error(json!(2), RpcError::new(UNAUTHORIZED, "bad secret"));
        let line = resp.to_line();
        assert!(line.contains("\"code\":-32001"));
        assert!(line.contains("\"message\":\"bad secret\""));
        assert!(!line.contains("\"result\""));
    }

    #[test]
    fn notification_has_no_id() {
        let n = Notification::new("state_changed", json!({"running": true}));
        let line = n.to_line();
        assert!(line.contains("\"method\":\"state_changed\""));
        assert!(!line.contains("\"id\""));
    }
}
