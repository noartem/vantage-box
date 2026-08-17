//! Проверка совместимости с конкретным бинарником sing-box.
//!
//! Набор проб прогоняет весь путь, которым пользуется приложение: сборка
//! рантайм-конфига, `sing-box check`, запуск, Clash API по HTTP и WebSocket,
//! переключение selector'а. Каждая проба записывает результат отдельно, ничего
//! не паникует — иначе матрицу совместимости не построить: нужно знать не
//! «работает / не работает», а что именно отвалилось.
//!
//! Изоляция обязательна: свой процесс sing-box, нестандартные порты, конфиг
//! без TUN (значит, без прав администратора и без вмешательства в сеть) и своя
//! рабочая папка. Рабочий sing-box пользователя не затрагивается.

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

/// Сколько ждём подъёма Clash API после запуска процесса.
const API_TIMEOUT: Duration = Duration::from_secs(20);
/// Сколько ждём первого сообщения в WebSocket.
const WS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    pub binary: PathBuf,
    /// Пустая папка под конфиги и состояние этого прогона.
    pub workdir: PathBuf,
    /// Порт Clash API. Нестандартный, чтобы не столкнуться с рабочим sing-box.
    pub api_port: u16,
    /// Порт локального mixed-инбаунда.
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
    /// Что именно получилось или почему не вышло.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    /// Версия, о которой сообщил сам бинарник.
    pub version: Option<String>,
    /// Как эта версия соотносится с задекларированным диапазоном.
    pub compatibility: Compatibility,
    pub checks: Vec<Check>,
    /// Все пробы прошли.
    pub ok: bool,
}

impl ProbeReport {
    pub fn failed(&self) -> Vec<&Check> {
        self.checks.iter().filter(|c| !c.ok).collect()
    }
}

/// Порядок проб = порядок столбцов в матрице.
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

/// Все пробы в порядке выполнения — нужен для заголовков таблицы.
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

/// Прогоняет все пробы. Никогда не паникует: отказ — это тоже результат.
pub async fn probe(options: &ProbeOptions) -> ProbeReport {
    let mut checks = Vec::new();
    let mut version = None;

    if let Err(e) = std::fs::create_dir_all(&options.workdir) {
        checks.push(fail(CHECK_PORTS, format!("не создать рабочую папку: {e}")));
        return finish(version, checks);
    }

    // 1. Порты. Чужой процесс мы не трогаем — просто не запускаемся.
    let busy: Vec<u16> = [options.api_port, options.mixed_port]
        .into_iter()
        .filter(|port| !port_is_free(*port))
        .collect();
    if busy.is_empty() {
        checks.push(pass(CHECK_PORTS, format!("{}, {} свободны", options.api_port, options.mixed_port)));
    } else {
        checks.push(fail(CHECK_PORTS, format!("заняты порты: {busy:?}")));
        return finish(version, checks);
    }

    // 2. Версия.
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

    // 3. Рантайм-конфиг.
    let user_config = options.workdir.join("user-config.json");
    let sample = sample_config(options.mixed_port);
    if let Err(e) = std::fs::write(&user_config, &sample) {
        checks.push(fail(CHECK_RUNTIME_CONFIG, format!("не записать конфиг: {e}")));
        return finish(version, checks);
    }

    let settings = probe_settings(options, &user_config);
    let prepared = match runtime::prepare_in(&options.workdir, &settings) {
        Ok(prepared) => {
            checks.push(pass(
                CHECK_RUNTIME_CONFIG,
                format!("clash_api на {}", prepared.external_controller),
            ));
            prepared
        }
        Err(e) => {
            checks.push(fail(CHECK_RUNTIME_CONFIG, e.to_string()));
            return finish(version, checks);
        }
    };

    // 4. Пользовательский конфиг обязан остаться байт в байт прежним.
    match std::fs::read_to_string(&user_config) {
        Ok(after) if after == sample => {
            checks.push(pass(CHECK_USER_CONFIG_INTACT, "не изменён".into()))
        }
        Ok(_) => checks.push(fail(CHECK_USER_CONFIG_INTACT, "файл изменён".into())),
        Err(e) => checks.push(fail(CHECK_USER_CONFIG_INTACT, e.to_string())),
    }

    // 5. sing-box check.
    match binary::check_config(&options.binary, Path::new(&prepared.config_path)) {
        Ok(result) if result.ok => checks.push(pass(CHECK_SINGBOX_CHECK, "конфиг принят".into())),
        Ok(result) => checks.push(fail(CHECK_SINGBOX_CHECK, first_line(&result.output))),
        Err(e) => checks.push(fail(CHECK_SINGBOX_CHECK, e.to_string())),
    }

    // 6. Запуск и Clash API.
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
            checks.push(fail(CHECK_API_UP, format!("не запустился: {e}")));
            return finish(version, checks);
        }
    };
    // С этого момента процесс гарантированно будет убит — даже при раннем return.
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
                format!("API не ответил за {} с", API_TIMEOUT.as_secs()),
            ));
            return finish(version, checks);
        }
    }

    // 7. Группы.
    let group_ok = match client.proxies().await {
        Ok(response) => match response.proxies.get("choose") {
            Some(group) if group.is_group() && group.is_selectable() => {
                checks.push(pass(
                    CHECK_PROXIES,
                    format!("группа choose, выбран {}", group.now.as_deref().unwrap_or("—")),
                ));
                true
            }
            Some(_) => {
                checks.push(fail(CHECK_PROXIES, "choose не распознан как selector".into()));
                false
            }
            None => {
                checks.push(fail(CHECK_PROXIES, "в ответе нет группы choose".into()));
                false
            }
        },
        Err(e) => {
            checks.push(fail(CHECK_PROXIES, e.to_string()));
            false
        }
    };

    // 8. Переключение.
    if group_ok {
        match select_and_verify(&client).await {
            Ok(detail) => checks.push(pass(CHECK_SELECT, detail)),
            Err(detail) => checks.push(fail(CHECK_SELECT, detail)),
        }
    } else {
        checks.push(fail(CHECK_SELECT, "пропущено: группы нет".into()));
    }

    // 9. WebSocket. /traffic приходит сам раз в секунду.
    match first_message(&client.ws_url("/traffic"), client.secret(), None).await {
        Ok(text) => checks.push(pass(CHECK_WS_TRAFFIC, truncate(&text, 60))),
        Err(e) => checks.push(fail(CHECK_WS_TRAFFIC, e)),
    }

    // 10. А /logs отдаёт только новые записи, поэтому активность создаём сами.
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

    // 11. /connections: список активных соединений отдаётся и парсится.
    match client.connections().await {
        Ok(snap) => checks.push(pass(
            CHECK_CONNECTIONS,
            format!("{} соединений, ↓{} ↑{}", snap.connections.len(), snap.download_total, snap.upload_total),
        )),
        Err(e) => checks.push(fail(CHECK_CONNECTIONS, e.to_string())),
    }

    // 12. Закрытие одного соединения по id: держим живой туннель к API-порту
    // (он заведомо доступен) и гасим именно его.
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

/// Конфиг без TUN: локальный mixed-порт и selector из двух `direct`.
/// Этого достаточно, чтобы проверить группы и переключение, ничего не меняя
/// в системе и не требуя прав администратора.
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
        Some(now) => Err(format!("после переключения выбран {now}")),
        None => Err("группа исчезла после переключения".into()),
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

/// Подключается к WebSocket и ждёт первое текстовое сообщение. Если задан
/// `poke_port`, после подписки стучится туда — так появляется запись в логе.
async fn first_message(
    url: &str,
    secret: &str,
    poke_port: Option<u16>,
) -> Result<String, String> {
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
    .ok_or_else(|| format!("нет сообщений за {} с", WS_TIMEOUT.as_secs()))
}

async fn connect_ws(url: &str, secret: &str) -> Result<WsStream, String> {
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

/// Открывает соединение к mixed-инбаунду, чтобы sing-box записал строку в лог.
/// Ответ не нужен — важен сам факт входящего соединения.
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

/// Держит живое соединение через mixed-инбаунд: CONNECT к локальной «мишени»
/// (свой `TcpListener`), которую sing-box дозванивает и через которую туннелирует.
/// Оба конца держим открытыми, пока не найдём соединение в `/connections` и не
/// погасим его по id — тогда проверяем, что оно исчезло из списка.
///
/// Раньше мишенью был сам API-порт, но sing-box не показывает в `/connections`
/// соединения к собственному `external_controller`, поэтому нужна отдельная
/// мишень.
async fn close_one_connection(
    client: &ClashClient,
    mixed_port: u16,
    _api_port: u16,
) -> Result<String, String> {
    use std::collections::HashSet;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    // Снимаем базовые id до того, как откроем своё соединение: так мы найдём
    // именно его, не привязываясь к схеме полей sing-box ( destinationPort и
    // прочее бывает то плоско, то вложенно в `metadata`).
    let baseline: HashSet<String> = client
        .connections()
        .await
        .map(|s| s.connections.into_iter().map(|c| c.id).collect())
        .unwrap_or_default();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("не открыть мишень: {e}"))?;
    let target_port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();

    // 1. Открываем туннель через mixed-инбаунд к нашей мишени.
    let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", mixed_port))
        .await
        .map_err(|e| format!("не подключиться к mixed: {e}"))?;
    let req = format!(
        "CONNECT 127.0.0.1:{target_port} HTTP/1.1\r\nHost: 127.0.0.1:{target_port}\r\n\r\n"
    );
    sock.write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    sock.flush().await.map_err(|e| e.to_string())?;

    // 2. Принимаем обратную сторону туннеля и держим её до конца проверки.
    let target = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .map_err(|_| "мишень не дождалась соединения от sing-box".to_string())?
        .map_err(|e| e.to_string())?
        .0;

    // 3. Ищем свежее соединение (id которого не было в базовом списке) и
    //    заодно проверяем, что вложенный `metadata` действительно разбирается:
    //    порт мишени должен совпасть — иначе модель расходится со схемой sing-box.
    let want = target_port.to_string();
    let mut last: Option<String> = None;
    let (id, meta_ok) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(snap) = client.connections().await {
                last = Some(format!(
                    "{} соединений: {:?}",
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
            "свежее соединение не появилось в списке; последний снимок: {}",
            last.unwrap_or_else(|| "<нет>".into())
        )
    })?;

    if !meta_ok {
        return Err(format!(
            "metadata.destinationPort не совпал с {want} — модель расходится со схемой sing-box"
        ));
    }

    // 4. Гасим и проверяем, что оно исчезло.
    client.close_connection(&id).await.map_err(|e| e.to_string())?;

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
        Ok(format!("id {} закрыт", truncate(&id, 12)))
    } else {
        Err("соединение не исчезло после DELETE".into())
    }
}

/// Убивает только наш дочерний процесс — и при обычном выходе, и при панике.
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
