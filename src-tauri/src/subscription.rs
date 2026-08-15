//! Подписки на списки прокси.
//!
//! Подписка — это URL, который отдаёт либо готовый конфиг sing-box (массив
//! `outbounds` или объект с `outbounds`), либо base64-список прокси-URI
//! (`ss://`, `vmess://`, `vless://`, `trojan://`, `hysteria2://`, `tuic://`).
//! Узлы вливаются в пользовательский `config.json` под тегами с префиксом
//! `sub:<id>:`, дописываются в selector/urltest-группы и применяются мягким
//! перезапуском с сохранением выбора.
//!
//! Префикс тега — это и есть учёт того, чем управляем: при обновлении все
//! `sub:`-теги снимаются и накатываются заново, так что повторное обновление
//! не плодит дубликаты. Пользовательский `config.json` переписывается
//! атомарно, с `.bak`; JSONC-комментарии при этом не сохраняются (как и в
//! редакторе конфига).

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::actions;
use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;
use crate::settings::{config_dir, Settings, SubscriptionSettings};
use crate::state::AppState;

/// Префикс тегов всех outbound'ов, внесённых подписками.
const TAG_PREFIX: &str = "sub:";

/// Имя sidecar-файла с состоянием подписок (хэши, время, ошибки).
const STATE_FILE: &str = "subscriptions-state.json";

/// Типы outbound'ов, которые не являются прокси-узлами: группы и псевдо-outbound'ы.
/// Их из подписки не вливаем.
const NON_PROXY_TYPES: &[&str] = &[
    "selector",
    "urltest",
    "direct",
    "reject",
    "block",
    "dns",
    "compatible",
    "dns-router",
];

// ---------------------------------------------------------------------------
// Публичный результат применения
// ---------------------------------------------------------------------------

/// Сводка по одной подписке после применения.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubUpdate {
    pub id: String,
    pub name: String,
    /// Сколько узлов влито.
    pub node_count: usize,
    /// Время обновления, unix-мс.
    pub last_updated: u64,
    /// Последняя ошибка, если не удалось обновить.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub updates: Vec<SubUpdate>,
    /// Изменился ли config.json (и был ли перезапуск).
    pub changed: bool,
    /// Был ли sing-box перезапущен.
    pub restarted: bool,
}

// ---------------------------------------------------------------------------
// Применение
// ---------------------------------------------------------------------------

/// Перетягивает все включённые подписки, вливает узлы в config и (если состав
/// изменился) мягко перезапускает sing-box.
pub async fn apply(app: &AppHandle, force: bool) -> Result<ApplyOutcome> {
    let state = app.state::<AppState>();
    let settings = state.settings.get();

    let enabled: Vec<SubscriptionSettings> = settings
        .subscriptions
        .iter()
        .filter(|s| s.enabled && !s.url.trim().is_empty())
        .cloned()
        .collect();

    // Сводка по каждой подписке: скачиваем и парсим.
    let mut fetched: Vec<(SubscriptionSettings, Vec<Value>, Option<String>)> = Vec::new();
    for sub in &enabled {
        match fetch_and_parse(&sub.url).await {
            Ok(outbounds) => fetched.push((sub.clone(), outbounds, None)),
            Err(e) => fetched.push((sub.clone(), Vec::new(), Some(e.to_string()))),
        }
    }

    // Подготавливаем узлы с тегами и считаем подпись, чтобы понять, было ли
    // изменение — лишний перезапуск sing-box не нужен.
    let prepared = prepare_outbounds(&fetched);
    let new_signature = signature(&prepared);

    let stored = load_state()?;
    // `None` — подписки никогда не применялись. Тогда трогаем config только
    // если есть что вливать: пустой набор на свежей установке не должен
    // перезаписывать конфиг и перезапускать sing-box.
    let changed = match &stored.signature {
        None => force || !prepared.is_empty(),
        Some(old) => force || new_signature != *old,
    };

    let mut updates = Vec::new();
    let now = now_millis();
    for (sub, _, err) in &fetched {
        let count = prepared
            .iter()
            .find(|(id, _, _)| id == &sub.id)
            .map(|(_, tags, _)| tags.len())
            .unwrap_or(0);
        updates.push(SubUpdate {
            id: sub.id.clone(),
            name: sub.name.clone(),
            node_count: count,
            last_updated: now,
            last_error: err.clone(),
        });
    }

    let mut restarted = false;
    if changed {
        let inject_settings = settings.clone();
        let inject_prepared = prepared.clone();
        let content = actions::blocking(move || inject_into_config(&inject_settings, &inject_prepared))
            .await?;

        // Фиксируем, что файл теперь содержит именно это — иначе watcher
        // решит, что конфиг правили снаружи.
        state.remember_config(&content);

        // Перезапуск только если sing-box уже работает: неработающий подхватит
        // новый конфиг при следующем старте.
        let running = actions::run_status(app).await?.running;
        if running {
            actions::restart(app).await?;
            restarted = true;
        }

        // Запоминаем новую подпись.
        let mut next = stored;
        next.signature = Some(new_signature);
        let _ = save_state(&next);
    }

    // В любом случае обновляем время/ошибки в sidecar (для UI).
    {
        let mut next = load_state()?;
        for u in &updates {
            next.entries.insert(
                u.id.clone(),
                SubStateEntry {
                    last_updated: u.last_updated,
                    node_count: u.node_count,
                    last_error: u.last_error.clone(),
                },
            );
        }
        let _ = save_state(&next);
    }

    Ok(ApplyOutcome {
        updates,
        changed,
        restarted,
    })
}

// ---------------------------------------------------------------------------
// Периодическое обновление
// ---------------------------------------------------------------------------

/// Фоновая задача: при старте однократно вливает узлы (если их ещё нет), затем
/// раз в минуту проверяет, не подошло ли время обновить какую-то подписку.
pub fn spawn_refresher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Стартовое применение безопасно: если набор узлов не изменился,
        // `apply` не трогает config и не перезапускает sing-box.
        let _ = apply(&app, false).await;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if any_due(&app) {
                let _ = apply(&app, false).await;
            }
        }
    });
}

/// Есть ли включённая подписка, у которой вышел `update_interval` с последнего
/// обновления. Ошибки чтения состояния трактуем как «пора обновить».
fn any_due(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let settings = state.settings.get();
    let now = now_millis();
    let stored = match load_state() {
        Ok(s) => s,
        Err(_) => return true,
    };
    settings
        .subscriptions
        .iter()
        .filter(|s| s.enabled && !s.url.trim().is_empty())
        .any(|s| {
            let interval_ms = s.update_interval.saturating_mul(3_600_000);
            if interval_ms == 0 {
                return false;
            }
            let last = stored.entries.get(&s.id).map(|e| e.last_updated).unwrap_or(0);
            now >= last + interval_ms
        })
}

// ---------------------------------------------------------------------------
// Подготовка outbound'ов: теги + подпись
// ---------------------------------------------------------------------------

/// `(id подписки, теги её узлов, готовые outbound'ы)`.
type Prepared = Vec<(String, Vec<String>, Vec<Value>)>;

fn prepare_outbounds(fetched: &[(SubscriptionSettings, Vec<Value>, Option<String>)]) -> Prepared {
    let mut all_tags: Vec<String> = Vec::new();
    let mut result: Prepared = Vec::new();

    for (sub, outbounds, _err) in fetched {
        let mut tags: Vec<String> = Vec::new();
        let mut prepared: Vec<Value> = Vec::new();
        let id_prefix = format!("{TAG_PREFIX}{}:", sub.id);

        for (i, ob) in outbounds.iter().enumerate() {
            let mut node = ob.clone();
            let remark = node
                .get("tag")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("node-{i}"));
            let base = format!("{id_prefix}{remark}");
            let tag = unique_tag(&base, &all_tags);
            node["tag"] = json!(tag);
            all_tags.push(tag.clone());
            tags.push(tag);
            prepared.push(node);
        }

        result.push((sub.id.clone(), tags, prepared));
    }

    result
}

fn unique_tag(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|t| t == base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !existing.iter().any(|t| *t == candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Стабильная подпись набора узлов: не зависит от порядка тегов.
fn signature(prepared: &Prepared) -> String {
    let mut hasher = Sha256::new();
    let mut lines: Vec<String> = Vec::new();
    for (id, _tags, outbounds) in prepared {
        let mut ob_lines: Vec<String> = outbounds
            .iter()
            .map(|o| {
                let mut sorted = o.clone();
                sort_object_keys(&mut sorted);
                serde_json::to_string(&sorted).unwrap_or_default()
            })
            .collect();
        ob_lines.sort();
        lines.push(format!("{id}|{}", ob_lines.join("\n")));
    }
    lines.sort();
    hasher.update(lines.join("\u{1f}").as_bytes());
    hex(&hasher.finalize())
}

fn sort_object_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // preserve_order включён, поэтому ключи сортируем явно для подписи.
            let mut entries: Vec<(String, Value)> = map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            map.clear();
            for (k, mut v) in entries {
                sort_object_keys(&mut v);
                map.insert(k, v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                sort_object_keys(v);
            }
        }
        _ => {}
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// Инъекция в config.json (блокирующая)
// ---------------------------------------------------------------------------

/// Вписывает подготовленные узлы в `config.json`, возвращает новое содержимое.
fn inject_into_config(settings: &Settings, prepared: &Prepared) -> Result<String> {
    let source = settings.sing_box.config_path.trim();
    if source.is_empty() {
        return Err(Error::Other(
            "путь к config.json sing-box не задан — укажите его в настройках".into(),
        ));
    }

    let raw = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let mut config: Value =
        serde_json::from_str(&strip_jsonc(&raw)).map_err(|e| Error::parse(source, e))?;

    let root = config
        .as_object_mut()
        .ok_or_else(|| Error::Other(format!("{source}: ожидался JSON-объект в корне")))?;

    let outbounds = root
        .entry("outbounds")
        .or_insert_with(|| json!([]));
    let outbounds = outbounds
        .as_array_mut()
        .ok_or_else(|| Error::Other("outbounds в config не является массивом".into()))?;

    // 1. Снимаем всё, что раньше внесли подписки.
    outbounds.retain(|o| !is_managed(o));
    for o in outbounds.iter_mut() {
        if let Some(list) = o.get_mut("outbounds").and_then(|v| v.as_array_mut()) {
            list.retain(|t| !t.as_str().is_some_and(|s| s.starts_with(TAG_PREFIX)));
        }
    }

    // 2. Целевые группы. Если хоть у одной включённой подписки задан
    //    `target_group`, вливаем только в перечисленные (и существующие)
    //    группы; иначе — во все selector/urltest.
    let explicit: Vec<String> = settings
        .subscriptions
        .iter()
        .filter(|s| s.enabled && !s.url.trim().is_empty())
        .filter_map(|s| s.target_group.clone())
        .filter(|t| !t.trim().is_empty())
        .collect();
    let target_tags: Vec<String> = if !explicit.is_empty() {
        let existing = selector_tags(outbounds);
        explicit
            .into_iter()
            .filter(|t| existing.iter().any(|e| e == t))
            .collect()
    } else {
        selector_tags(outbounds)
    };

    // 3. Добавляем узлы и собираем теги по целевым группам.
    let mut tags_by_group: HashMap<String, Vec<String>> = HashMap::new();
    for (_id, tags, nodes) in prepared {
        for node in nodes {
            outbounds.push(node.clone());
        }
        for group_tag in &target_tags {
            tags_by_group
                .entry(group_tag.clone())
                .or_default()
                .extend(tags.iter().cloned());
        }
    }

    // 4. Дописываем теги в группы (без дубликатов).
    for o in outbounds.iter_mut() {
        let kind = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let tag = o.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !matches!(kind, "selector" | "urltest") {
            continue;
        }
        if let Some(add) = tags_by_group.get(&tag) {
            let list = o
                .get_mut("outbounds")
                .and_then(|v| v.as_array_mut())
                .ok_or_else(|| Error::Other(format!("группа {tag}: поле outbounds не массив")))?;
            for new_tag in add {
                let exists = list.iter().any(|t| t.as_str() == Some(new_tag.as_str()));
                if !exists {
                    list.push(json!(new_tag));
                }
            }
        }
    }

    let body = serde_json::to_string_pretty(&config)
        .map_err(|e| Error::parse(source, e))?;
    let body = format!("{body}\n");

    write_config_atomic(Path::new(source), &body)?;
    Ok(body)
}

/// Теги всех selector/urltest-групп в `outbounds`, в порядке объявления.
fn selector_tags(outbounds: &[Value]) -> Vec<String> {
    outbounds
        .iter()
        .filter_map(|o| {
            let kind = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(kind, "selector" | "urltest") {
                o.get("tag").and_then(|v| v.as_str()).map(String::from)
            } else {
                None
            }
        })
        .collect()
}

fn is_managed(o: &Value) -> bool {
    o.get("tag")
        .and_then(|t| t.as_str())
        .is_some_and(|t| t.starts_with(TAG_PREFIX))
}

/// Атомарная запись с `.bak`-бэкапом — как в `write_singbox_config`.
fn write_config_atomic(path: &Path, body: &str) -> Result<()> {
    if path.is_file() {
        let backup = path.with_extension("json.bak");
        std::fs::copy(path, &backup)
            .map_err(|e| Error::io(backup.display().to_string(), e))?;
    }
    let tmp = path.with_extension("json.vbtmp");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Скачивание и разбор
// ---------------------------------------------------------------------------

async fn fetch_and_parse(url: &str) -> Result<Vec<Value>> {
    let text = fetch(url).await?;
    parse_content(&text)
}

async fn fetch(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .no_proxy()
        .build()
        .map_err(|e| Error::Transport(e.to_string()))?;
    let resp = client
        .get(url.trim())
        .header("User-Agent", "vantage-box")
        .send()
        .await
        .map_err(|e| Error::Transport(format!("подписка недоступна: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Other(format!(
            "подписка вернула {status}"
        )));
    }
    resp.text().await.map_err(|e| Error::Transport(e.to_string()))
}

fn parse_content(text: &str) -> Result<Vec<Value>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::Other("подписка пуста".into()));
    }

    // 1. Готовый sing-box: объект с outbounds или массив outbound'ов.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        let outbounds = extract_singbox_outbounds(v);
        let proxies: Vec<Value> = outbounds.into_iter().filter(is_proxy_outbound).collect();
        if !proxies.is_empty() {
            return Ok(proxies);
        }
        return Err(Error::Other(
            "в подписке-конфиге sing-box нет прокси-outbound'ов".into(),
        ));
    }

    // 2. Может быть base64 — разворачиваем и пробуем как список URI.
    //    Раскодированное должно содержать `://`, иначе это не список URI
    //    (а, например, просто текст) — тогда пробуем исходник как есть.
    let lines_text = match b64_decode(trimmed) {
        Some(d) if d.contains("://") => d,
        _ => trimmed.to_string(),
    };

    let mut outbounds = Vec::new();
    for line in lines_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_uri(line) {
            Ok(o) => outbounds.push(o),
            Err(_) => continue,
        }
    }
    if outbounds.is_empty() {
        Err(Error::Other(
            "не удалось разобрать подписку: нет ни одного понятного узла".into(),
        ))
    } else {
        Ok(outbounds)
    }
}

fn extract_singbox_outbounds(v: Value) -> Vec<Value> {
    match v {
        Value::Object(map) => {
            if let Some(Value::Array(arr)) = map.get("outbounds") {
                arr.clone()
            } else {
                Vec::new()
            }
        }
        Value::Array(arr) => arr,
        _ => Vec::new(),
    }
}

fn is_proxy_outbound(o: &Value) -> bool {
    let kind = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
    !NON_PROXY_TYPES.iter().any(|t| t.eq_ignore_ascii_case(kind))
}

// ---------------------------------------------------------------------------
// Парсинг URI
// ---------------------------------------------------------------------------

fn parse_uri(uri: &str) -> Result<Value> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| Error::Other("не похоже на прокси-URI".into()))?;
    let scheme = scheme.to_lowercase();
    match scheme.as_str() {
        "ss" => parse_ss(rest),
        "vmess" => parse_vmess(rest),
        "vless" => parse_vless(rest),
        "trojan" => parse_trojan(rest),
        "hysteria2" | "hy2" => parse_hysteria2(rest),
        "tuic" => parse_tuic(rest),
        other => Err(Error::Other(format!("схема {other} не поддерживается"))),
    }
}

/// `rest` без `scheme://`: разделяем на `body`, `query`, `fragment`.
fn split_uri(rest: &str) -> (&str, &str, &str) {
    let (main, fragment) = match rest.split_once('#') {
        Some((m, f)) => (m, f),
        None => (rest, ""),
    };
    let (body, query) = match main.split_once('?') {
        Some((b, q)) => (b, q),
        None => (main, ""),
    };
    (body, query, fragment)
}

/// Тег из `#fragment` (URL-декодируем минимально).
fn tag_from_fragment(fragment: &str, fallback: &str) -> String {
    let raw = percent_decode(fragment);
    if raw.trim().is_empty() {
        fallback.to_string()
    } else {
        raw
    }
}

/// `host:port` → `(host, port)`.
fn split_host_port(s: &str) -> Result<(String, u16)> {
    let (host, port) = s
        .rsplit_once(':')
        .ok_or_else(|| Error::Other(format!("ожидался host:port, получилось «{s}»")))?;
    let port: u16 = port
        .parse()
        .map_err(|_| Error::Other(format!("некорректный порт «{port}»")))?;
    Ok((host.to_string(), port))
}

/// Простая query-пара: `?a=b&c=d` → HashMap.
fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(percent_decode(k), percent_decode(v));
        } else if !pair.is_empty() {
            map.insert(percent_decode(pair), String::new());
        }
    }
    map
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Пробуем несколько вариантов base64 (standard/url-safe, с паддингом и без).
/// Возвращает строку, если раскодировалось в корректный UTF-8 — без проверки
/// содержимого: у ss это `method:password`, у vmess — JSON, и только у целой
/// подписки — список URI с `://`.
fn b64_decode(input: &str) -> Option<String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    for engine in [&URL_SAFE_NO_PAD, &URL_SAFE, &STANDARD_NO_PAD, &STANDARD] {
        if let Ok(bytes) = engine.decode(&cleaned) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Some(text);
            }
        }
    }
    None
}

// --- конкретные схемы -----------------------------------------------------

fn parse_ss(rest: &str) -> Result<Value> {
    let (body, _query, fragment) = split_uri(rest);
    // SIP002: userinfo@host:port ; legacy: base64(method:password@host:port)
    let (userinfo, hostport) = if let Some((u, h)) = body.split_once('@') {
        (u.to_string(), h.to_string())
    } else {
        // legacy — всё base64
        let decoded = b64_decode(body).unwrap_or_else(|| body.to_string());
        let (u, h) = decoded
            .split_once('@')
            .ok_or_else(|| Error::Other("ss: не удалось разобрать legacy-формат".into()))?;
        (u.to_string(), h.to_string())
    };

    // userinfo может быть base64url(method:password) или явным method:password.
    let credentials = if userinfo.contains(':') {
        userinfo
    } else {
        b64_decode(&userinfo).unwrap_or(userinfo)
    };
    let (method, password) = credentials
        .split_once(':')
        .ok_or_else(|| Error::Other("ss: ожидалось method:password".into()))?;
    let (host, port) = split_host_port(&hostport)?;
    let (host, port) = strip_brackets(host, port);

    Ok(json!({
        "type": "shadowsocks",
        "tag": tag_from_fragment(fragment, &format!("ss-{host}")),
        "server": host,
        "server_port": port,
        "method": method,
        "password": password,
    }))
}

fn parse_vmess(rest: &str) -> Result<Value> {
    let decoded = b64_decode(rest)
        .ok_or_else(|| Error::Other("vmess: ожидался base64".into()))?;
    let v: Value = serde_json::from_str(&decoded)
        .map_err(|e| Error::Other(format!("vmess: некорректный JSON — {e}")))?;

    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    // vmess-провайдеры шлюют port и aid то числом, то строкой — принимаем оба.
    let i = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse::<i64>().ok())))
            .unwrap_or(0)
    };

    let server = s("add");
    let port = i("port") as u16;
    let (server, port) = strip_brackets(server, port);

    let mut outbound = json!({
        "type": "vmess",
        "tag": s("ps"),
        "server": server,
        "server_port": port,
        "uuid": s("id"),
        "alter_id": i("aid"),
    });

    // Транспорт.
    let net = s("net");
    if !net.is_empty() && net != "tcp" {
        if let Some(transport) = vmess_transport(&net, &v) {
            outbound["transport"] = transport;
        }
    }

    // TLS.
    if s("tls") == "tls" || !s("sni").is_empty() {
        let mut tls = Map::new();
        tls.insert("enabled".into(), json!(true));
        let sni = s("sni");
        if !sni.is_empty() {
            tls.insert("server_name".into(), json!(sni));
        }
        if s("allowInsecure") == "true" || s("verify_cert") == "false" {
            tls.insert("insecure".into(), json!(true));
        }
        let alpn = s("alpn");
        if !alpn.is_empty() {
            tls.insert("alpn".into(), json!(alpn.split(',').map(String::from).collect::<Vec<_>>()));
        }
        outbound["tls"] = Value::Object(tls);
    }

    Ok(outbound)
}

fn vmess_transport(net: &str, v: &Value) -> Option<Value> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    match net {
        "ws" => Some(json!({
            "type": "ws",
            "path": s("path"),
            "headers": if s("host").is_empty() { json!({}) } else { json!({ "Host": s("host") }) },
        })),
        "grpc" => Some(json!({
            "type": "grpc",
            "service_name": s("path"),
        })),
        "h2" | "http" => {
            let mut headers = Map::new();
            let host = s("host");
            if !host.is_empty() {
                headers.insert("Host".into(), json!(host));
            }
            Some(json!({
                "type": "http",
                "path": s("path"),
                "host": if host.is_empty() { Vec::<String>::new() } else { vec![host] },
            }))
        }
        _ => None,
    }
}

fn parse_vless(rest: &str) -> Result<Value> {
    let (body, query, fragment) = split_uri(rest);
    let (uuid, hostport) = body
        .split_once('@')
        .ok_or_else(|| Error::Other("vless: ожидался uuid@host:port".into()))?;
    let (host, port) = split_host_port(hostport)?;
    let (host, port) = strip_brackets(host, port);
    let q = parse_query(query);

    let mut outbound = json!({
        "type": "vless",
        "tag": tag_from_fragment(fragment, &format!("vless-{host}")),
        "server": host,
        "server_port": port,
        "uuid": percent_decode(uuid),
    });

    let flow = q.get("flow").cloned().unwrap_or_default();
    if !flow.is_empty() {
        outbound["flow"] = json!(flow);
    }

    // TLS / Reality.
    let security = q.get("security").cloned().unwrap_or_default();
    let sni = q.get("sni").cloned().unwrap_or_default();
    let fp = q.get("fp").cloned().unwrap_or_default();
    if security == "reality" {
        let mut tls = Map::new();
        tls.insert("enabled".into(), json!(true));
        if !sni.is_empty() {
            tls.insert("server_name".into(), json!(sni));
        }
        let pbk = q.get("pbk").cloned().unwrap_or_default();
        let sid = q.get("sid").cloned().unwrap_or_default();
        let mut reality = Map::new();
        reality.insert("enabled".into(), json!(true));
        reality.insert("public_key".into(), json!(pbk));
        if !sid.is_empty() {
            reality.insert("short_id".into(), json!(sid));
        }
        tls.insert("reality".into(), Value::Object(reality));
        if !fp.is_empty() {
            tls.insert("utls".into(), json!({ "enabled": true, "fingerprint": fp }));
        }
        outbound["tls"] = Value::Object(tls);
    } else if security == "tls" || !sni.is_empty() {
        let mut tls = Map::new();
        tls.insert("enabled".into(), json!(true));
        if !sni.is_empty() {
            tls.insert("server_name".into(), json!(sni));
        }
        if q.get("insecure").map(|v| v == "1").unwrap_or(false) {
            tls.insert("insecure".into(), json!(true));
        }
        if !fp.is_empty() {
            tls.insert("utls".into(), json!({ "enabled": true, "fingerprint": fp }));
        }
        outbound["tls"] = Value::Object(tls);
    }

    // Транспорт.
    let net = q.get("type").cloned().unwrap_or_default();
    if net == "ws" {
        let path = q.get("path").cloned().unwrap_or_default();
        let host = q.get("host").cloned().unwrap_or_default();
        let mut headers = Map::new();
        if !host.is_empty() {
            headers.insert("Host".into(), json!(host));
        }
        outbound["transport"] = json!({
            "type": "ws",
            "path": percent_decode(&path),
            "headers": Value::Object(headers),
        });
    } else if net == "grpc" {
        let sn = q.get("serviceName").cloned().unwrap_or_default();
        outbound["transport"] = json!({ "type": "grpc", "service_name": sn });
    } else if net == "http" || net == "h2" {
        let path = q.get("path").cloned().unwrap_or_default();
        let host = q.get("host").cloned().unwrap_or_default();
        outbound["transport"] = json!({
            "type": "http",
            "path": percent_decode(&path),
            "host": if host.is_empty() { Vec::<String>::new() } else { vec![host] },
        });
    }

    Ok(outbound)
}

fn parse_trojan(rest: &str) -> Result<Value> {
    let (body, query, fragment) = split_uri(rest);
    let (password, hostport) = body
        .split_once('@')
        .ok_or_else(|| Error::Other("trojan: ожидался password@host:port".into()))?;
    let (host, port) = split_host_port(hostport)?;
    let (host, port) = strip_brackets(host, port);
    let q = parse_query(query);
    let sni = q.get("sni").cloned().unwrap_or_default();

    let mut tls = Map::new();
    tls.insert("enabled".into(), json!(true));
    if !sni.is_empty() {
        tls.insert("server_name".into(), json!(sni));
    }
    if q.get("allowInsecure").map(|v| v == "1").unwrap_or(false) {
        tls.insert("insecure".into(), json!(true));
    }

    let mut outbound = json!({
        "type": "trojan",
        "tag": tag_from_fragment(fragment, &format!("trojan-{host}")),
        "server": host,
        "server_port": port,
        "password": percent_decode(password),
        "tls": Value::Object(tls),
    });

    let net = q.get("type").cloned().unwrap_or_default();
    if net == "ws" {
        let path = q.get("path").cloned().unwrap_or_default();
        let host = q.get("host").cloned().unwrap_or_default();
        outbound["transport"] = json!({
            "type": "ws",
            "path": percent_decode(&path),
            "headers": if host.is_empty() { json!({}) } else { json!({ "Host": host }) },
        });
    } else if net == "grpc" {
        let sn = q.get("serviceName").cloned().unwrap_or_default();
        outbound["transport"] = json!({ "type": "grpc", "service_name": sn });
    }

    Ok(outbound)
}

fn parse_hysteria2(rest: &str) -> Result<Value> {
    let (body, query, fragment) = split_uri(rest);
    // hysteria2://password@host:port  или  hysteria2://host:port?auth=...
    let (password, hostport) = match body.split_once('@') {
        Some((p, h)) => (Some(p.to_string()), h.to_string()),
        None => (None, body.to_string()),
    };
    let (host, port) = split_host_port(&hostport)?;
    let (host, port) = strip_brackets(host, port);
    let q = parse_query(query);
    let sni = q.get("sni").cloned().unwrap_or_default();

    let mut tls = Map::new();
    tls.insert("enabled".into(), json!(true));
    if !sni.is_empty() {
        tls.insert("server_name".into(), json!(sni));
    }
    if q.get("insecure").map(|v| v == "1").unwrap_or(false) {
        tls.insert("insecure".into(), json!(true));
    }

    let mut outbound = json!({
        "type": "hysteria2",
        "tag": tag_from_fragment(fragment, &format!("hy2-{host}")),
        "server": host,
        "server_port": port,
        "tls": Value::Object(tls),
    });

    if let Some(pw) = password {
        outbound["password"] = json!(percent_decode(&pw));
    } else if let Some(auth) = q.get("auth") {
        outbound["password"] = json!(auth);
    }

    let obfs = q.get("obfs").cloned().unwrap_or_default();
    if !obfs.is_empty() {
        let mut obfs_map = Map::new();
        obfs_map.insert("type".into(), json!(obfs));
        if let Some(pw) = q.get("obfs-password") {
            obfs_map.insert("password".into(), json!(pw));
        }
        outbound["obfs"] = Value::Object(obfs_map);
    }

    Ok(outbound)
}

fn parse_tuic(rest: &str) -> Result<Value> {
    let (body, query, fragment) = split_uri(rest);
    let (credentials, hostport) = body
        .split_once('@')
        .ok_or_else(|| Error::Other("tuic: ожидался uuid:password@host:port".into()))?;
    let (uuid, password) = credentials
        .split_once(':')
        .ok_or_else(|| Error::Other("tuic: ожидался uuid:password".into()))?;
    let (host, port) = split_host_port(hostport)?;
    let (host, port) = strip_brackets(host, port);
    let q = parse_query(query);
    let sni = q.get("sni").cloned().unwrap_or_default();

    let mut tls = Map::new();
    tls.insert("enabled".into(), json!(true));
    if !sni.is_empty() {
        tls.insert("server_name".into(), json!(sni));
    }
    if q.get("allow_insecure").map(|v| v == "1").unwrap_or(false) {
        tls.insert("insecure".into(), json!(true));
    }
    let alpn = q.get("alpn").cloned().unwrap_or_default();
    if !alpn.is_empty() {
        tls.insert("alpn".into(), json!(alpn.split(',').map(String::from).collect::<Vec<_>>()));
    }

    let mut outbound = json!({
        "type": "tuic",
        "tag": tag_from_fragment(fragment, &format!("tuic-{host}")),
        "server": host,
        "server_port": port,
        "uuid": percent_decode(uuid),
        "password": percent_decode(password),
        "tls": Value::Object(tls),
    });

    let cc = q.get("congestion_control").cloned().unwrap_or_default();
    if !cc.is_empty() {
        outbound["congestion_control"] = json!(cc);
    }

    Ok(outbound)
}

/// Убирает квадратные скобки у IPv6-хоста.
fn strip_brackets(host: String, port: u16) -> (String, u16) {
    let trimmed = host.trim_start_matches('[').trim_end_matches(']');
    (trimmed.to_string(), port)
}

// ---------------------------------------------------------------------------
// Состояние подписок (sidecar)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubscriptionsState {
    /// Подпись последнего влитого набора — чтобы не перезапускать без нужды.
    pub signature: Option<String>,
    pub entries: HashMap<String, SubStateEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubStateEntry {
    pub last_updated: u64,
    pub node_count: usize,
    pub last_error: Option<String>,
}

impl Default for SubStateEntry {
    fn default() -> Self {
        Self {
            last_updated: 0,
            node_count: 0,
            last_error: None,
        }
    }
}

fn state_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join(STATE_FILE))
}

pub fn load_state() -> Result<SubscriptionsState> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(SubscriptionsState::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
    if raw.trim().is_empty() {
        return Ok(SubscriptionsState::default());
    }
    serde_json::from_str(&raw).map_err(|e| Error::parse(path.display().to_string(), e))
}

pub fn save_state(state: &SubscriptionsState) -> Result<()> {
    let path = state_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let body = serde_json::to_string_pretty(state).unwrap_or_default();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Генерирует короткий стабильный идентификатор для новой подписки.
pub fn new_id() -> String {
    let mut buf = [0u8; 4];
    if getrandom::fill(&mut buf).is_err() {
        // Фолбэк: псевдо-ид из времени. На практике не нужен.
        return format!("{:x}", now_millis());
    }
    hex(&buf)
}

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(v: &Value) -> String {
        v.get("tag").and_then(|t| t.as_str()).unwrap_or("").to_string()
    }

    #[test]
    fn parses_ss_sip002() {
        // base64url("aes-256-gcm:password") = YWVzLTI1Ni1nY206cGFzc3dvcmQ
        let uri = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:8388#MyNode";
        let o = parse_uri(uri).unwrap();
        assert_eq!(o["type"], "shadowsocks");
        assert_eq!(o["server"], "example.com");
        assert_eq!(o["server_port"], 8388);
        assert_eq!(o["method"], "aes-256-gcm");
        assert_eq!(o["password"], "password");
        assert_eq!(tag(&o), "MyNode");
    }

    #[test]
    fn parses_vmess() {
        let json = r#"{"v":"2","ps":"JP","add":"1.2.3.4","port":"443","id":"uuid-here","aid":"0","net":"ws","type":"none","host":"example.com","path":"/ray","tls":"tls","sni":"example.com"}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        let uri = format!("vmess://{b64}");
        let o = parse_uri(&uri).unwrap();
        assert_eq!(o["type"], "vmess");
        assert_eq!(o["server"], "1.2.3.4");
        assert_eq!(o["server_port"], 443);
        assert_eq!(o["uuid"], "uuid-here");
        assert_eq!(o["transport"]["type"], "ws");
        assert_eq!(o["transport"]["path"], "/ray");
        assert_eq!(o["tls"]["enabled"], true);
        assert_eq!(tag(&o), "JP");
    }

    #[test]
    fn parses_vless_reality() {
        let uri = "vless://uuid@host.example:443?type=ws&security=reality&sni=sni.com&pbk=PUB&sid=ab&fp=chrome&path=%2Fpath#Node";
        let o = parse_uri(uri).unwrap();
        assert_eq!(o["type"], "vless");
        assert_eq!(o["uuid"], "uuid");
        assert_eq!(o["tls"]["reality"]["enabled"], true);
        assert_eq!(o["tls"]["reality"]["public_key"], "PUB");
        assert_eq!(o["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(o["transport"]["type"], "ws");
        assert_eq!(o["transport"]["path"], "/path");
        assert_eq!(tag(&o), "Node");
    }

    #[test]
    fn parses_trojan() {
        let uri = "trojan://pass@host:443?sni=host#T";
        let o = parse_uri(uri).unwrap();
        assert_eq!(o["type"], "trojan");
        assert_eq!(o["password"], "pass");
        assert_eq!(o["tls"]["server_name"], "host");
        assert_eq!(tag(&o), "T");
    }

    #[test]
    fn parses_hysteria2() {
        let uri = "hysteria2://secret@host:443?sni=host&insecure=1#H";
        let o = parse_uri(uri).unwrap();
        assert_eq!(o["type"], "hysteria2");
        assert_eq!(o["password"], "secret");
        assert_eq!(o["tls"]["insecure"], true);
        assert_eq!(tag(&o), "H");
    }

    #[test]
    fn parses_tuic() {
        let uri = "tuic://uid:pwd@host:443?sni=host&congestion_control=bbr#U";
        let o = parse_uri(uri).unwrap();
        assert_eq!(o["type"], "tuic");
        assert_eq!(o["uuid"], "uid");
        assert_eq!(o["password"], "pwd");
        assert_eq!(o["congestion_control"], "bbr");
    }

    #[test]
    fn parses_base64_uri_list() {
        let lines = "trojan://pass@host:443?sni=host#A\nvless://uuid@host2:443#B\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(lines);
        let out = parse_content(&b64).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["type"], "trojan");
        assert_eq!(out[1]["type"], "vless");
    }

    #[test]
    fn parses_singbox_json() {
        let cfg = r#"{"outbounds":[{"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}},{"type":"selector","tag":"main","outbounds":["X"]}]}"#;
        let out = parse_content(cfg).unwrap();
        // selector отфильтрован — остаётся только прокси.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["type"], "trojan");
    }

    fn mksub(id: &str) -> SubscriptionSettings {
        SubscriptionSettings {
            id: id.into(),
            name: id.into(),
            url: "http://x".into(),
            enabled: true,
            target_group: None,
            update_interval: 24,
        }
    }

    #[test]
    fn signature_stable_regardless_of_order() {
        // Две подписки с одним узлом каждая. Меняем порядок — подпись набора
        // не должна зависеть от того, в каком порядке подписки пришли.
        let ob_a = vec![json!({"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}})];
        let ob_b = vec![json!({"type":"vless","tag":"Y","server":"h2","server_port":1,"uuid":"u"})];
        let p1 = prepare_outbounds(&[
            (mksub("a"), ob_a.clone(), None),
            (mksub("b"), ob_b.clone(), None),
        ]);
        let p2 = prepare_outbounds(&[
            (mksub("b"), ob_b, None),
            (mksub("a"), ob_a, None),
        ]);
        assert_eq!(signature(&p1), signature(&p2));
    }

    #[test]
    fn signature_detects_change() {
        let ob = vec![json!({"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}})];
        let p = prepare_outbounds(&[(mksub("a"), ob, None)]);
        let ob2 = vec![json!({"type":"vless","tag":"X","server":"h","server_port":1,"uuid":"u"})];
        let p2 = prepare_outbounds(&[(mksub("a"), ob2, None)]);
        assert_ne!(signature(&p), signature(&p2));
    }

    #[test]
    fn prepare_outbounds_tags_with_prefix() {
        let ob = vec![json!({"type":"trojan","tag":"MyNode","server":"h","server_port":1,"password":"p","tls":{"enabled":true}})];
        let prepared = prepare_outbounds(&[(mksub("a"), ob, None)]);
        let (_, tags, nodes) = &prepared[0];
        assert_eq!(tags.len(), 1);
        assert!(tags[0].starts_with("sub:a:"));
        assert!(tags[0].ends_with("MyNode"));
        assert_eq!(nodes[0]["tag"], tags[0]);
    }
}