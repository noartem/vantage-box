//! Compatibility check against a specific sing-box binary.
//!
//! A set of probes exercises the whole path the app uses: building the runtime
//! config, `sing-box check`, startup, the Clash API over HTTP and WebSocket,
//! selector switching. Each probe records its result separately and never
//! panics — otherwise the compatibility matrix could not be built: we need to
//! know not "works / does not work", but what exactly broke.
//!
//! Isolation is mandatory: its own sing-box process, non-standard ports, a
//! config without TUN (so no admin rights and no network interference), and
//! its own working folder. The user's working sing-box is not touched.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::binary;
use crate::clash::client::{compatibility, normalize_version, ClashClient};
use crate::clash::models::Compatibility;
use crate::runtime;
use crate::settings::{ClashApiSettings, Settings, SingBoxSettings};

/// How long we wait for the Clash API to come up after starting the process.
const API_TIMEOUT: Duration = Duration::from_secs(20);
/// How long we wait for the first WebSocket message.
const WS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    pub binary: PathBuf,
    /// An empty folder for the configs and state of this run.
    pub workdir: PathBuf,
    /// The Clash API port. Non-standard, to avoid clashing with the working sing-box.
    pub api_port: u16,
    /// The local mixed-inbound port.
    pub mixed_port: u16,
}

impl ProbeOptions {
    pub fn new(binary: impl Into<PathBuf>, workdir: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            workdir: workdir.into(),
            api_port: 19090,
            mixed_port: 19080,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub ok: bool,
    /// What exactly happened or why it did not work out.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    /// The version the binary itself reported.
    pub version: Option<String>,
    /// How this version relates to the declared range.
    pub compatibility: Compatibility,
    pub checks: Vec<Check>,
    /// All probes passed.
    pub ok: bool,
}

impl ProbeReport {
    pub fn failed(&self) -> Vec<&Check> {
        self.checks.iter().filter(|c| !c.ok).collect()
    }
}

/// Probe order = column order in the matrix.
const CHECK_PORTS: &str = "ports";
const CHECK_VERSION: &str = "version";
const CHECK_RUNTIME_CONFIG: &str = "runtime-config";
const CHECK_USER_CONFIG_INTACT: &str = "user-config-intact";
const CHECK_SINGBOX_CHECK: &str = "sing-box-check";
const CHECK_API_UP: &str = "api-up";
const CHECK_PROXIES: &str = "proxies";
const CHECK_SELECT: &str = "select";
const CHECK_WS_TRAFFIC: &str = "ws-traffic";
const CHECK_WS_LOGS: &str = "ws-logs";
const CHECK_CONNECTIONS: &str = "connections";
const CHECK_CLOSE_CONNECTION: &str = "close-connection";

/// All probes in execution order — needed for the table headers.
pub const CHECK_ORDER: &[&str] = &[
    CHECK_PORTS,
    CHECK_VERSION,
    CHECK_RUNTIME_CONFIG,
    CHECK_USER_CONFIG_INTACT,
    CHECK_SINGBOX_CHECK,
    CHECK_API_UP,
    CHECK_PROXIES,
    CHECK_SELECT,
    CHECK_WS_TRAFFIC,
    CHECK_WS_LOGS,
    CHECK_CONNECTIONS,
    CHECK_CLOSE_CONNECTION,
];

/// Runs all probes. Never panics: a failure is also a result.
pub async fn probe(options: &ProbeOptions) -> ProbeReport {
    let mut checks = Vec::new();
    let mut version = None;

    if let Err(e) = std::fs::create_dir_all(&options.workdir) {
        checks.push(fail(
            CHECK_PORTS,
            format!("could not create the working folder: {e}"),
        ));
        return finish(version, checks);
    }

    // 1. Ports. We do not touch a foreign process — we simply do not start.
    let busy: Vec<u16> = [options.api_port, options.mixed_port]
        .into_iter()
        .filter(|port| !port_is_free(*port))
        .collect();
    if busy.is_empty() {
        checks.push(pass(
            CHECK_PORTS,
            format!("{}, {} are free", options.api_port, options.mixed_port),
        ));
    } else {
        checks.push(fail(CHECK_PORTS, format!("ports busy: {busy:?}")));
        return finish(version, checks);
    }

    // 2. Version.
    match binary::detect_version(&options.binary) {
        Ok(v) => {
            checks.push(pass(CHECK_VERSION, v.clone()));
            version = Some(v);
        }
        Err(e) => {
            checks.push(fail(CHECK_VERSION, e.to_string()));
            return finish(version, checks);
        }
    }

    // 3. Runtime config.
    let user_config = options.workdir.join("user-config.json");
    let sample = sample_config(options.mixed_port);
    if let Err(e) = std::fs::write(&user_config, &sample) {
        checks.push(fail(
            CHECK_RUNTIME_CONFIG,
            format!("could not write the config: {e}"),
        ));
        return finish(version, checks);
    }

    let settings = probe_settings(options, &user_config);
    let prepared = match runtime::prepare_in(&options.workdir, &settings) {
        Ok(prepared) => {
            checks.push(pass(
                CHECK_RUNTIME_CONFIG,
                format!("clash_api at {}", prepared.external_controller),
            ));
            prepared
        }
        Err(e) => {
            checks.push(fail(CHECK_RUNTIME_CONFIG, e.to_string()));
            return finish(version, checks);
        }
    };

    // 4. The user config must stay byte-for-byte the same.
    match std::fs::read_to_string(&user_config) {
        Ok(after) if after == sample => {
            checks.push(pass(CHECK_USER_CONFIG_INTACT, "unchanged".into()))
        }
        Ok(_) => checks.push(fail(CHECK_USER_CONFIG_INTACT, "file changed".into())),
        Err(e) => checks.push(fail(CHECK_USER_CONFIG_INTACT, e.to_string())),
    }

    // 5. sing-box check.
    match binary::check_config(&options.binary, Path::new(&prepared.config_path)) {
        Ok(result) if result.ok => checks.push(pass(CHECK_SINGBOX_CHECK, "config accepted".into())),
        Ok(result) => checks.push(fail(CHECK_SINGBOX_CHECK, first_line(&result.output))),
        Err(e) => checks.push(fail(CHECK_SINGBOX_CHECK, e.to_string())),
    }

    // 6. Startup and the Clash API.
    let data_dir = options.workdir.join("data");
    let _ = std::fs::create_dir_all(&data_dir);

    let child = Command::new(&options.binary)
        .args([
            "run",
            "-c",
            &prepared.config_path,
            "-D",
            &data_dir.display().to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    let child = match child {
        Ok(child) => child,
        Err(e) => {
            checks.push(fail(CHECK_API_UP, format!("did not start: {e}")));
            return finish(version, checks);
        }
    };
    // From this point the process is guaranteed to be killed — even on an early return.
    let _guard = ChildGuard(child);

    let api = ClashApiSettings {
        url: settings.clash_api.url.clone(),
        secret: prepared.secret.clone(),
        ..Default::default()
    };
    let client = match ClashClient::new(&api) {
        Ok(client) => client,
        Err(e) => {
            checks.push(fail(CHECK_API_UP, e.to_string()));
            return finish(version, checks);
        }
    };

    match wait_for_api(&client, API_TIMEOUT).await {
        Some(reported) => checks.push(pass(CHECK_API_UP, format!("/version → {reported}"))),
        None => {
            checks.push(fail(
                CHECK_API_UP,
                format!("API did not respond within {} s", API_TIMEOUT.as_secs()),
            ));
            return finish(version, checks);
        }
    }

    // 7. Groups.
    let group_ok = match client.proxies().await {
        Ok(response) => match response.proxies.get("choose") {
            Some(group) if group.is_group() && group.is_selectable() => {
                checks.push(pass(
                    CHECK_PROXIES,
                    format!(
                        "group choose, selected {}",
                        group.now.as_deref().unwrap_or("—")
                    ),
                ));
                true
            }
            Some(_) => {
                checks.push(fail(
                    CHECK_PROXIES,
                    "choose not recognized as a selector".into(),
                ));
                false
            }
            None => {
                checks.push(fail(
                    CHECK_PROXIES,
                    "no choose group in the response".into(),
                ));
                false
            }
        },
        Err(e) => {
            checks.push(fail(CHECK_PROXIES, e.to_string()));
            false
        }
    };

    // 8. Switching.
    if group_ok {
        match select_and_verify(&client).await {
            Ok(detail) => checks.push(pass(CHECK_SELECT, detail)),
            Err(detail) => checks.push(fail(CHECK_SELECT, detail)),
        }
    } else {
        checks.push(fail(CHECK_SELECT, "skipped: no group".into()));
    }

    // 9. WebSocket. /traffic arrives on its own once a second.
    match first_message(&client.ws_url("/traffic"), client.secret(), None).await {
        Ok(text) => checks.push(pass(CHECK_WS_TRAFFIC, truncate(&text, 60))),
        Err(e) => checks.push(fail(CHECK_WS_TRAFFIC, e)),
    }

    // 10. But /logs only returns new entries, so we create the activity ourselves.
    let poke = Some(options.mixed_port);
    match first_message(
        &format!("{}?level=info", client.ws_url("/logs")),
        client.secret(),
        poke,
    )
    .await
    {
        Ok(text) => checks.push(pass(CHECK_WS_LOGS, truncate(&text, 60))),
        Err(e) => checks.push(fail(CHECK_WS_LOGS, e)),
    }

    // 11. /connections: the list of active connections is returned and parsed.
    match client.connections().await {
        Ok(snap) => checks.push(pass(
            CHECK_CONNECTIONS,
            format!(
                "{} connections, ↓{} ↑{}",
                snap.connections.len(),
                snap.download_total,
                snap.upload_total
            ),
        )),
        Err(e) => checks.push(fail(CHECK_CONNECTIONS, e.to_string())),
    }

    // 12. Closing one connection by id: keep a live tunnel to the API port (it
    // is guaranteed to be reachable) and kill exactly that one.
    match close_one_connection(&client, options.mixed_port, options.api_port).await {
        Ok(detail) => checks.push(pass(CHECK_CLOSE_CONNECTION, detail)),
        Err(detail) => checks.push(fail(CHECK_CLOSE_CONNECTION, detail)),
    }

    finish(version, checks)
}

// ---------------------------------------------------------------------------

fn probe_settings(options: &ProbeOptions, user_config: &Path) -> Settings {
    Settings {
        sing_box: SingBoxSettings {
            config_path: user_config.display().to_string(),
            binary_path: options.binary.display().to_string(),
            ..Default::default()
        },
        clash_api: ClashApiSettings {
            url: format!("http://127.0.0.1:{}", options.api_port),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A config without TUN: a local mixed port and a selector of two `direct`s.
/// That is enough to check groups and switching, changing nothing in the
/// system and requiring no admin rights.
pub fn sample_config(mixed_port: u16) -> String {
    format!(
        r#"{{
  "log": {{ "level": "info", "timestamp": true }},
  "inbounds": [
    {{
      "type": "mixed",
      "tag": "test-in",
      "listen": "127.0.0.1",
      "listen_port": {mixed_port}
    }}
  ],
  "outbounds": [
    {{ "type": "direct", "tag": "direct-a" }},
    {{ "type": "direct", "tag": "direct-b" }},
    {{
      "type": "selector",
      "tag": "choose",
      "outbounds": ["direct-a", "direct-b"],
      "default": "direct-a"
    }}
  ],
  "route": {{ "final": "choose" }}
}}
"#
    )
}

async fn select_and_verify(client: &ClashClient) -> Result<String, String> {
    client
        .select("choose", "direct-b")
        .await
        .map_err(|e| e.to_string())?;

    let after = client.proxies().await.map_err(|e| e.to_string())?;
    match after.proxies.get("choose").and_then(|g| g.now.clone()) {
        Some(now) if now == "direct-b" => Ok("direct-a → direct-b".into()),
        Some(now) => Err(format!("after switching, {now} is selected")),
        None => Err("the group disappeared after switching".into()),
    }
}

async fn wait_for_api(client: &ClashClient, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(info) = client.version().await {
            return Some(normalize_version(&info.version));
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connects to a WebSocket and waits for the first text message. If `poke_port`
/// is set, after subscribing it pokes it — so an entry appears in the log.
async fn first_message(url: &str, secret: &str, poke_port: Option<u16>) -> Result<String, String> {
    let mut stream = connect_ws(url, secret).await?;

    if let Some(port) = poke_port {
        poke_inbound(port).await;
    }

    tokio::time::timeout(WS_TIMEOUT, async {
        while let Some(Ok(message)) = stream.next().await {
            if let Message::Text(text) = message {
                return Some(text.to_string());
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
    .ok_or_else(|| format!("no messages within {} s", WS_TIMEOUT.as_secs()))
}

async fn connect_ws(url: &str, secret: &str) -> Result<WsStream, String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    if !secret.is_empty() {
        let value = format!("Bearer {secret}")
            .parse()
            .map_err(|_| "invalid secret".to_string())?;
        request.headers_mut().insert("Authorization", value);
    }
    let (stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Opens a connection to the mixed inbound so sing-box writes a line to the log.
/// No response is needed — the fact of an incoming connection is what matters.
async fn poke_inbound(port: u16) {
    use tokio::io::AsyncWriteExt;

    let Ok(mut socket) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
        return;
    };
    let _ = socket
        .write_all(
            b"CONNECT vantage-box.invalid:443 HTTP/1.1\r\nHost: vantage-box.invalid:443\r\n\r\n",
        )
        .await;
    let _ = socket.flush().await;
}

/// Keeps a live connection through the mixed inbound: a CONNECT to a local
/// "target" (our own `TcpListener`) that sing-box dials and tunnels through.
/// We keep both ends open until we find the connection in `/connections` and
/// kill it by id — then we check that it disappeared from the list.
///
/// Previously the target was the API port itself, but sing-box does not show
/// connections to its own `external_controller` in `/connections`, so a
/// separate target is needed.
async fn close_one_connection(
    client: &ClashClient,
    mixed_port: u16,
    _api_port: u16,
) -> Result<String, String> {
    use std::collections::HashSet;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    // Snapshot the baseline ids before we open our own connection: that way we
    // find exactly it, without depending on the shape of sing-box fields
    // (destinationPort and friends are sometimes flat, sometimes nested in
    // `metadata`).
    let baseline: HashSet<String> = client
        .connections()
        .await
        .map(|s| s.connections.into_iter().map(|c| c.id).collect())
        .unwrap_or_default();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not open the target: {e}"))?;
    let target_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    // 1. Open a tunnel through the mixed inbound to our target.
    let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", mixed_port))
        .await
        .map_err(|e| format!("could not connect to mixed: {e}"))?;
    let req = format!(
        "CONNECT 127.0.0.1:{target_port} HTTP/1.1\r\nHost: 127.0.0.1:{target_port}\r\n\r\n"
    );
    sock.write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    sock.flush().await.map_err(|e| e.to_string())?;

    // 2. Accept the other end of the tunnel and hold it until the end of the check.
    let target = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .map_err(|_| "the target did not get a connection from sing-box".to_string())?
        .map_err(|e| e.to_string())?
        .0;

    // 3. Look for a fresh connection (whose id was not in the baseline) and at
    //    the same time verify that the nested `metadata` really parses: the
    //    target port must match — otherwise the model diverges from the
    //    sing-box schema.
    let want = target_port.to_string();
    let mut last: Option<String> = None;
    let (id, meta_ok) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(snap) = client.connections().await {
                last = Some(format!(
                    "{} connections: {:?}",
                    snap.connections.len(),
                    snap.connections
                        .iter()
                        .map(|c| {
                            (
                                c.id.clone(),
                                c.metadata.host.clone(),
                                c.metadata.destination_port.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                ));
                if let Some(c) = snap.connections.iter().find(|c| !baseline.contains(&c.id)) {
                    return (c.id.clone(), c.metadata.destination_port == want);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .ok()
    .ok_or_else(|| {
        format!(
            "a fresh connection did not appear in the list; last snapshot: {}",
            last.unwrap_or_else(|| "<none>".into())
        )
    })?;

    if !meta_ok {
        return Err(format!(
            "metadata.destinationPort did not match {want} — the model diverges from the sing-box schema"
        ));
    }

    // 4. Kill it and verify it disappeared.
    client
        .close_connection(&id)
        .await
        .map_err(|e| e.to_string())?;

    let gone = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.connections().await {
                Ok(snap) if !snap.connections.iter().any(|c| c.id == id) => {
                    return Ok::<_, String>(())
                }
                Ok(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(e) => return Err(e.to_string()),
            }
        }
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .is_some();

    drop(sock);
    drop(target);

    if gone {
        Ok(format!("id {} closed", truncate(&id, 12)))
    } else {
        Err("the connection did not disappear after DELETE".into())
    }
}

/// Kills only our child process — both on a normal exit and on a panic.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port_is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn pass(name: &str, detail: String) -> Check {
    Check {
        name: name.into(),
        ok: true,
        detail,
    }
}

fn fail(name: &str, detail: String) -> Check {
    Check {
        name: name.into(),
        ok: false,
        detail,
    }
}

fn finish(version: Option<String>, checks: Vec<Check>) -> ProbeReport {
    ProbeReport {
        compatibility: version
            .as_deref()
            .map(compatibility)
            .unwrap_or(Compatibility::Unknown),
        ok: checks.iter().all(|c| c.ok),
        version,
        checks,
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}
