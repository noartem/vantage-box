//! Subscriptions to proxy lists.
//!
//! A subscription is a URL that returns either a ready sing-box config (an
//! `outbounds` array or an object with `outbounds`), or a base64 list of
//! proxy URIs (`ss://`, `vmess://`, `vless://`, `trojan://`, `hysteria2://`,
//! `tuic://`). Nodes are injected into the user's `config.json` under tags
//! prefixed with `sub:<id>:`, appended to selector/urltest groups, and
//! applied via a soft restart that preserves the selection.
//!
//! The tag prefix is how we keep track of what we manage: on update all
//! `sub:` tags are removed and re-added, so a repeated update does not grow
//! duplicates. The user's `config.json` is rewritten atomically with a `.bak`;
//! JSONC comments are not preserved (same as in the config editor).

use std::collections::{HashMap, HashSet};
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

/// The tag prefix for all outbounds contributed by subscriptions.
const TAG_PREFIX: &str = "sub:";

/// Name of the sidecar file with subscription state (hashes, times, errors).
const STATE_FILE: &str = "subscriptions-state.json";

/// Outbound types that are not proxy nodes: groups and pseudo-outbounds.
/// We do not inject these from a subscription as nodes — but `selector` and
/// `urltest` are kept as *groups* (see [`is_group_outbound`]).
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
// Public apply result
// ---------------------------------------------------------------------------

/// Summary of one subscription after applying.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubUpdate {
    pub id: String,
    pub name: String,
    /// How many nodes were injected.
    pub node_count: usize,
    /// Update time, unix-ms.
    pub last_updated: u64,
    /// The last error, if the update failed.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub updates: Vec<SubUpdate>,
    /// Whether config.json changed (and a restart happened).
    pub changed: bool,
    /// Whether sing-box was restarted.
    pub restarted: bool,
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// Pulls all enabled subscriptions, injects nodes into the config, and (if the
/// set of nodes changed) softly restarts sing-box.
pub async fn apply(app: &AppHandle, force: bool) -> Result<ApplyOutcome> {
    let state = app.state::<AppState>();
    let settings = state.settings.get();

    let enabled: Vec<SubscriptionSettings> = settings
        .subscriptions
        .iter()
        .filter(|s| s.enabled && !s.url.trim().is_empty())
        .cloned()
        .collect();

    // Summary per subscription: download and parse.
    let mut fetched: Vec<(SubscriptionSettings, ParsedConfig, Option<String>)> = Vec::new();
    for sub in &enabled {
        match fetch_and_parse(&sub.url).await {
            Ok(outbounds) => fetched.push((sub.clone(), outbounds, None)),
            Err(e) => fetched.push((sub.clone(), ParsedConfig::default(), Some(e.to_string()))),
        }
    }

    // Prepare nodes with tags and compute a signature, to know whether anything
    // changed — an extra sing-box restart is not needed.
    let prepared = prepare_outbounds(&fetched);
    let new_signature = signature(&prepared);

    let stored = load_state()?;
    // `None` — subscriptions were never applied. Then we touch the config only
    // if there is something to inject: an empty set on a fresh install must not
    // overwrite the config and restart sing-box.
    let changed = match &stored.signature {
        None => force || !prepared.is_empty(),
        Some(old) => force || new_signature != *old,
    };

    let mut updates = Vec::new();
    let now = now_millis();
    for (sub, _, err) in &fetched {
        let count = prepared
            .iter()
            .find(|(id, _)| id == &sub.id)
            .map(|(_, s)| s.nodes.len() + s.endpoints.len())
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
        let prev_created = stored.created_groups.clone();
        let (content, created) = actions::blocking(move || {
            inject_into_config(&inject_settings, &inject_prepared, &prev_created)
        })
        .await?;

        // Record that the file now contains exactly this — otherwise the
        // watcher would think the config was edited externally.
        state.remember_config(&content);

        // Restart only if sing-box is already running: a non-running one picks
        // up the new config on the next start.
        let running = actions::run_status(app).await?.running;
        if running {
            actions::restart(app).await?;
            restarted = true;
        }

        // Save the new signature and the groups we created this round. The
        // saved settings are now reflected in the running config, so there is
        // nothing pending to apply.
        let mut next = stored.clone();
        next.signature = Some(new_signature);
        next.created_groups = created;
        next.apply_pending = false;
        let _ = save_state(&next);
    }

    // In any case update the time/errors in the sidecar (for the UI).
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
// Periodic refresh
// ---------------------------------------------------------------------------

/// Background task: at startup injects the nodes once (if not present yet),
/// then once a minute checks whether any subscription is due for an update.
pub fn spawn_refresher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // The startup apply is safe: if the set of nodes has not changed,
        // `apply` does not touch the config and does not restart sing-box.
        let _ = apply(&app, false).await;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if any_due(&app) {
                let _ = apply(&app, false).await;
            }
        }
    });
}

/// Whether an enabled subscription has passed its `update_interval` since the
/// last update. Read errors of the state are treated as "time to update".
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
            let last = stored
                .entries
                .get(&s.id)
                .map(|e| e.last_updated)
                .unwrap_or(0);
            now >= last + interval_ms
        })
}

// ---------------------------------------------------------------------------
// Preparing outbounds: tags + signature
// ---------------------------------------------------------------------------

/// What a subscription yields after parsing, before retagging.
#[derive(Clone, Default)]
struct ParsedConfig {
    /// Proxy outbounds (shadowsocks, vmess, …) — not groups.
    nodes: Vec<Value>,
    /// Entries from the top-level `endpoints` array (wireguard, tailscale).
    endpoints: Vec<Value>,
    /// selector/urltest outbounds the subscription ships.
    groups: Vec<Value>,
}

/// One subscription's contribution after retagging: nodes and endpoints retagged
/// with `sub:<id>:` and groups with their `outbounds` references rewritten to the
/// retagged tags (the groups' own tags stay unchanged).
#[derive(Clone, Default)]
struct ParsedSub {
    nodes: Vec<Value>,
    endpoints: Vec<Value>,
    groups: Vec<Value>,
}

/// `(subscription id, prepared contribution)`.
type Prepared = Vec<(String, ParsedSub)>;

fn prepare_outbounds(fetched: &[(SubscriptionSettings, ParsedConfig, Option<String>)]) -> Prepared {
    let mut all_tags: Vec<String> = Vec::new();
    let mut result: Prepared = Vec::new();

    for (sub, cfg, _err) in fetched {
        let id_prefix = format!("{TAG_PREFIX}{}:", sub.id);
        // original tag → retagged tag, for rewriting group references.
        let mut retag: HashMap<String, String> = HashMap::new();

        let mut nodes = Vec::with_capacity(cfg.nodes.len());
        for (i, ob) in cfg.nodes.iter().enumerate() {
            let (node, orig) = retag_item(ob, &id_prefix, &format!("node-{i}"), &mut all_tags);
            if !orig.is_empty() {
                retag.insert(
                    orig,
                    node.get("tag")
                        .and_then(|t| t.as_str())
                        .unwrap()
                        .to_string(),
                );
            }
            nodes.push(node);
        }

        let mut endpoints = Vec::with_capacity(cfg.endpoints.len());
        for (i, ep) in cfg.endpoints.iter().enumerate() {
            let (ep, orig) = retag_item(ep, &id_prefix, &format!("endpoint-{i}"), &mut all_tags);
            if !orig.is_empty() {
                retag.insert(
                    orig,
                    ep.get("tag").and_then(|t| t.as_str()).unwrap().to_string(),
                );
            }
            endpoints.push(ep);
        }

        // Rewrite each group's `outbounds` references via the retag map. References
        // to other groups (e.g. a manual selector → the auto urltest) and to the
        // user's own outbounds (direct/block) are not in the map and stay as-is.
        let mut groups = Vec::with_capacity(cfg.groups.len());
        for g in &cfg.groups {
            let mut group = g.clone();
            if let Some(list) = group.get_mut("outbounds").and_then(|v| v.as_array_mut()) {
                for entry in list.iter_mut() {
                    if let Some(s) = entry.as_str() {
                        if let Some(new) = retag.get(s) {
                            *entry = json!(new);
                        }
                    }
                }
            }
            groups.push(group);
        }

        result.push((
            sub.id.clone(),
            ParsedSub {
                nodes,
                endpoints,
                groups,
            },
        ));
    }

    result
}

/// Clones `item`, rewrites its `tag` to a unique `sub:`-prefixed tag, and returns
/// it together with the original tag (empty if the item had none).
fn retag_item(
    item: &Value,
    id_prefix: &str,
    fallback: &str,
    all_tags: &mut Vec<String>,
) -> (Value, String) {
    let mut node = item.clone();
    let orig = node.get("tag").and_then(|t| t.as_str()).map(String::from);
    let remark = orig.clone().unwrap_or_else(|| fallback.to_string());
    let base = format!("{id_prefix}{remark}");
    let tag = unique_tag(&base, all_tags);
    node["tag"] = json!(tag);
    all_tags.push(tag.clone());
    (node, orig.unwrap_or_default())
}

fn unique_tag(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|t| t == base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// A stable signature of the whole contributed set (nodes + endpoints + groups):
/// does not depend on the order of tags or of subscriptions.
fn signature(prepared: &Prepared) -> String {
    let mut hasher = Sha256::new();
    let mut lines: Vec<String> = Vec::new();
    for (id, sub) in prepared {
        let node_lines = bucket_lines(&sub.nodes);
        let ep_lines = bucket_lines(&sub.endpoints);
        let grp_lines = bucket_lines(&sub.groups);
        lines.push(format!(
            "{id}|N:{node_lines}\u{1e}E:{ep_lines}\u{1e}G:{grp_lines}",
            node_lines = node_lines.join("\n"),
            ep_lines = ep_lines.join("\n"),
            grp_lines = grp_lines.join("\n"),
        ));
    }
    lines.sort();
    hasher.update(lines.join("\u{1f}").as_bytes());
    hex(&hasher.finalize())
}

/// Sorted, key-normalized JSON strings of a bucket — order-independent.
fn bucket_lines(items: &[Value]) -> Vec<String> {
    let mut v: Vec<String> = items
        .iter()
        .map(|o| {
            let mut sorted = o.clone();
            sort_object_keys(&mut sorted);
            serde_json::to_string(&sorted).unwrap_or_default()
        })
        .collect();
    v.sort();
    v
}

fn sort_object_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // preserve_order is enabled, so we sort keys explicitly for the signature.
            let mut entries: Vec<(String, Value)> =
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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
// Injecting into config.json (blocking)
// ---------------------------------------------------------------------------

/// Writes the prepared nodes/endpoints/groups into `config.json`. Returns the new
/// contents and the tags of the groups this call *created* (so they can be
/// tracked and removed on a later update — see [`SubscriptionsState::created_groups`]).
fn inject_into_config(
    settings: &Settings,
    prepared: &Prepared,
    prev_created: &[String],
) -> Result<(String, Vec<String>)> {
    let source = settings.sing_box.config_path.trim();
    if source.is_empty() {
        return Err(Error::Other(
            "config.json path is not set — specify it in settings".into(),
        ));
    }

    let raw = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let mut config: Value =
        serde_json::from_str(&strip_jsonc(&raw)).map_err(|e| Error::parse(source, e))?;

    let root = config
        .as_object_mut()
        .ok_or_else(|| Error::Other(format!("{source}: expected a JSON object at the root")))?;

    // Take the arrays out of the object so we can mutate them without borrow
    // conflicts (outbounds + endpoints are touched together in the inject loop).
    let mut outbounds: Vec<Value> = match root.remove("outbounds") {
        Some(Value::Array(a)) => a,
        Some(_) => {
            return Err(Error::Other(
                "outbounds in the config is not an array".into(),
            ))
        }
        None => Vec::new(),
    };
    let any_endpoints = prepared.iter().any(|(_, s)| !s.endpoints.is_empty());
    let had_endpoints = root.contains_key("endpoints");
    let mut endpoints: Vec<Value> = Vec::new();
    let mut preserve_endpoints: Option<Value> = None;
    match root.remove("endpoints") {
        None => {}
        Some(Value::Array(a)) => endpoints = a,
        Some(other) => {
            if any_endpoints {
                return Err(Error::Other(
                    "endpoints in the config is not an array".into(),
                ));
            }
            // We do not touch endpoints — keep the original value as-is.
            preserve_endpoints = Some(other);
        }
    }

    // 1. Remove everything that subscriptions previously contributed.
    //    - managed nodes (sub: prefix) from outbounds and endpoints;
    //    - sub: references from every remaining group's/member's `outbounds` list;
    //    - groups we created last time (recorded in `prev_created`). Groups the
    //      user owns and we only *filled* are left in place — stripping their sub:
    //      refs empties them back to the user's original state.
    outbounds.retain(|o| !is_managed(o));
    endpoints.retain(|o| !is_managed(o));
    for o in outbounds.iter_mut() {
        strip_managed_refs(o);
    }
    for o in endpoints.iter_mut() {
        strip_managed_refs(o);
    }
    let prev_created_set: HashSet<&str> = prev_created.iter().map(String::as_str).collect();
    outbounds.retain(|o| {
        let tag = o.get("tag").and_then(|t| t.as_str()).unwrap_or("");
        !(is_group_outbound(o) && prev_created_set.contains(tag))
    });

    // 2. Target groups for node-only subscriptions (those that brought no groups of
    //    their own). If any of them has a `target_group`, pour only into the listed
    //    (and existing) groups; otherwise — into all selector/urltest groups.
    //    Subscriptions that ship their own groups ignore `target_group`.
    let has_groups: HashMap<&str, bool> = prepared
        .iter()
        .map(|(id, s)| (id.as_str(), !s.groups.is_empty()))
        .collect();
    let existing_groups = selector_tags(&outbounds);
    let explicit: Vec<String> = settings
        .subscriptions
        .iter()
        .filter(|s| s.enabled && !s.url.trim().is_empty())
        .filter(|s| !has_groups.get(s.id.as_str()).copied().unwrap_or(false))
        .filter_map(|s| s.target_group.clone())
        .filter(|t| !t.trim().is_empty())
        .collect();
    let target_tags: Vec<String> = if !explicit.is_empty() {
        explicit
            .into_iter()
            .filter(|t| existing_groups.iter().any(|e| e == t))
            .collect()
    } else {
        existing_groups
    };

    // 3. Inject.
    let mut created_groups: Vec<String> = Vec::new();
    for (_id, s) in prepared {
        for node in &s.nodes {
            outbounds.push(node.clone());
        }
        for ep in &s.endpoints {
            endpoints.push(ep.clone());
        }

        if s.groups.is_empty() {
            // Node-only: pour node tags into the target groups (dedup).
            let node_tags: Vec<String> = s
                .nodes
                .iter()
                .filter_map(|n| n.get("tag").and_then(|t| t.as_str()).map(String::from))
                .collect();
            for group_tag in &target_tags {
                append_refs_to_group(&mut outbounds, group_tag, &node_tags)?;
            }
        } else {
            // Group-aware: insert each group, or fill an existing one with the
            // same name.
            for g in &s.groups {
                let tag = g
                    .get("tag")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                if tag.is_empty() {
                    continue;
                }
                let new_refs: Vec<String> = g
                    .get("outbounds")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if group_exists(&outbounds, &tag) {
                    append_refs_to_group(&mut outbounds, &tag, &new_refs)?;
                } else if tag_taken(&outbounds, &tag) {
                    // A non-group outbound already holds this tag — do not create
                    // a duplicate (sing-box tags must be unique).
                    continue;
                } else {
                    outbounds.push(g.clone());
                    created_groups.push(tag);
                }
            }
        }
    }

    // Put the arrays back.
    root.insert("outbounds".to_string(), Value::Array(outbounds));
    match preserve_endpoints {
        Some(v) => {
            root.insert("endpoints".to_string(), v);
        }
        None => {
            if had_endpoints || !endpoints.is_empty() {
                root.insert("endpoints".to_string(), Value::Array(endpoints));
            }
        }
    }

    let body = serde_json::to_string_pretty(&config).map_err(|e| Error::parse(source, e))?;
    let body = format!("{body}\n");

    write_config_atomic(Path::new(source), &body)?;
    Ok((body, created_groups))
}

/// Tags of all selector/urltest groups in `outbounds`, in declaration order.
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

/// Removes `sub:`-prefixed entries from an outbound's/member's `outbounds` list.
fn strip_managed_refs(o: &mut Value) {
    if let Some(list) = o.get_mut("outbounds").and_then(|v| v.as_array_mut()) {
        list.retain(|t| !t.as_str().is_some_and(|s| s.starts_with(TAG_PREFIX)));
    }
}

/// Whether an outbound is a group (selector/urltest).
fn is_group_outbound(o: &Value) -> bool {
    let kind = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
    matches!(kind, "selector" | "urltest")
}

/// Whether a selector/urltest group with `tag` exists in `outbounds`.
fn group_exists(outbounds: &[Value], tag: &str) -> bool {
    outbounds
        .iter()
        .any(|o| is_group_outbound(o) && o.get("tag").and_then(|t| t.as_str()) == Some(tag))
}

/// Whether any outbound (group or node) already holds `tag`.
fn tag_taken(outbounds: &[Value], tag: &str) -> bool {
    outbounds
        .iter()
        .any(|o| o.get("tag").and_then(|t| t.as_str()) == Some(tag))
}

/// Appends `refs` to the selector/urltest group named `group_tag` (dedup). Does
/// nothing if no such group exists. Errors if the group's `outbounds` is not an
/// array (malformed config).
fn append_refs_to_group(outbounds: &mut [Value], group_tag: &str, refs: &[String]) -> Result<()> {
    for o in outbounds.iter_mut() {
        if !is_group_outbound(o) {
            continue;
        }
        if o.get("tag").and_then(|t| t.as_str()) != Some(group_tag) {
            continue;
        }
        let list = o
            .get_mut("outbounds")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| {
                Error::Other(format!(
                    "group {group_tag}: outbounds field is not an array"
                ))
            })?;
        for new_tag in refs {
            let exists = list.iter().any(|t| t.as_str() == Some(new_tag.as_str()));
            if !exists {
                list.push(json!(new_tag));
            }
        }
        return Ok(());
    }
    Ok(())
}

/// Atomic write with a `.bak` backup — same as in `write_singbox_config`.
fn write_config_atomic(path: &Path, body: &str) -> Result<()> {
    if path.is_file() {
        let backup = path.with_extension("json.bak");
        std::fs::copy(path, &backup).map_err(|e| Error::io(backup.display().to_string(), e))?;
    }
    let tmp = path.with_extension("json.vbtmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Downloading and parsing
// ---------------------------------------------------------------------------

async fn fetch_and_parse(url: &str) -> Result<ParsedConfig> {
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
        .map_err(|e| Error::Transport(format!("subscription unavailable: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Other(format!("subscription returned {status}")));
    }
    resp.text()
        .await
        .map_err(|e| Error::Transport(e.to_string()))
}

fn parse_content(text: &str) -> Result<ParsedConfig> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::Other("subscription is empty".into()));
    }

    // 1. A ready sing-box config: an object with `outbounds` (and optionally
    //    `endpoints`), or a bare array of outbounds.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        let cfg = extract_singbox_config(v);
        if cfg.nodes.is_empty() && cfg.endpoints.is_empty() {
            return Err(Error::Other(
                "the sing-box config subscription has no proxy outbounds".into(),
            ));
        }
        return Ok(cfg);
    }

    // 2. May be base64 — decode and try as a URI list. The decoded text must
    //    contain `://`, otherwise it is not a URI list (for example, just text)
    //    — then try the source as-is.
    let lines_text = match b64_decode(trimmed) {
        Some(d) if d.contains("://") => d,
        _ => trimmed.to_string(),
    };

    let mut nodes = Vec::new();
    for line in lines_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_uri(line) {
            Ok(o) => nodes.push(o),
            Err(_) => continue,
        }
    }
    if nodes.is_empty() {
        Err(Error::Other(
            "could not parse the subscription: no recognizable node".into(),
        ))
    } else {
        Ok(ParsedConfig {
            nodes,
            endpoints: Vec::new(),
            groups: Vec::new(),
        })
    }
}

/// Splits a ready sing-box config into proxy nodes, endpoints, and groups.
fn extract_singbox_config(v: Value) -> ParsedConfig {
    let (outbounds, endpoints) = match v {
        Value::Object(map) => {
            let outbounds = map
                .get("outbounds")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let endpoints = map
                .get("endpoints")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            (outbounds, endpoints)
        }
        Value::Array(arr) => (arr, Vec::new()),
        _ => (Vec::new(), Vec::new()),
    };

    let mut nodes = Vec::new();
    let mut groups = Vec::new();
    for ob in outbounds {
        if is_group_outbound(&ob) {
            groups.push(ob);
        } else if is_proxy_outbound(&ob) {
            nodes.push(ob);
        }
        // direct/block/dns/… — dropped, as before.
    }

    ParsedConfig {
        nodes,
        endpoints,
        groups,
    }
}

fn is_proxy_outbound(o: &Value) -> bool {
    let kind = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
    !NON_PROXY_TYPES.iter().any(|t| t.eq_ignore_ascii_case(kind))
}

// ---------------------------------------------------------------------------
// Parsing URIs
// ---------------------------------------------------------------------------

fn parse_uri(uri: &str) -> Result<Value> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| Error::Other("does not look like a proxy URI".into()))?;
    let scheme = scheme.to_lowercase();
    match scheme.as_str() {
        "ss" => parse_ss(rest),
        "vmess" => parse_vmess(rest),
        "vless" => parse_vless(rest),
        "trojan" => parse_trojan(rest),
        "hysteria2" | "hy2" => parse_hysteria2(rest),
        "tuic" => parse_tuic(rest),
        other => Err(Error::Other(format!("scheme {other} is not supported"))),
    }
}

/// `rest` without `scheme://`: split into `body`, `query`, `fragment`.
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

/// A tag from `#fragment` (URL-decode minimally).
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
        .ok_or_else(|| Error::Other(format!("expected host:port, got \"{s}\"")))?;
    let port: u16 = port
        .parse()
        .map_err(|_| Error::Other(format!("invalid port \"{port}\"")))?;
    Ok((host.to_string(), port))
}

/// Simple query pairs: `?a=b&c=d` → HashMap.
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

/// Try several base64 variants (standard/url-safe, with and without padding).
/// Returns the string if it decoded to valid UTF-8 — without checking the
/// contents: for ss it is `method:password`, for vmess it is JSON, and only
/// for a whole subscription is it a list of URIs with `://`.
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

// --- specific schemes -------------------------------------------------------

fn parse_ss(rest: &str) -> Result<Value> {
    let (body, _query, fragment) = split_uri(rest);
    // SIP002: userinfo@host:port ; legacy: base64(method:password@host:port)
    let (userinfo, hostport) = if let Some((u, h)) = body.split_once('@') {
        (u.to_string(), h.to_string())
    } else {
        // legacy — everything is base64
        let decoded = b64_decode(body).unwrap_or_else(|| body.to_string());
        let (u, h) = decoded
            .split_once('@')
            .ok_or_else(|| Error::Other("ss: could not parse the legacy format".into()))?;
        (u.to_string(), h.to_string())
    };

    // userinfo may be base64url(method:password) or an explicit method:password.
    let credentials = if userinfo.contains(':') {
        userinfo
    } else {
        b64_decode(&userinfo).unwrap_or(userinfo)
    };
    let (method, password) = credentials
        .split_once(':')
        .ok_or_else(|| Error::Other("ss: expected method:password".into()))?;
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
    let decoded = b64_decode(rest).ok_or_else(|| Error::Other("vmess: expected base64".into()))?;
    let v: Value = serde_json::from_str(&decoded)
        .map_err(|e| Error::Other(format!("vmess: invalid JSON — {e}")))?;

    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    // vmess providers send port and aid sometimes as a number, sometimes as a
    // string — accept both.
    let i = |k: &str| {
        v.get(k)
            .and_then(|x| {
                x.as_i64()
                    .or_else(|| x.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
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

    // Transport.
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
            tls.insert(
                "alpn".into(),
                json!(alpn.split(',').map(String::from).collect::<Vec<_>>()),
            );
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
        .ok_or_else(|| Error::Other("vless: expected uuid@host:port".into()))?;
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

    // Transport.
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
        .ok_or_else(|| Error::Other("trojan: expected password@host:port".into()))?;
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
    // hysteria2://password@host:port  or  hysteria2://host:port?auth=...
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
        .ok_or_else(|| Error::Other("tuic: expected uuid:password@host:port".into()))?;
    let (uuid, password) = credentials
        .split_once(':')
        .ok_or_else(|| Error::Other("tuic: expected uuid:password".into()))?;
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
        tls.insert(
            "alpn".into(),
            json!(alpn.split(',').map(String::from).collect::<Vec<_>>()),
        );
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

/// Strips square brackets from an IPv6 host.
fn strip_brackets(host: String, port: u16) -> (String, u16) {
    let trimmed = host.trim_start_matches('[').trim_end_matches(']');
    (trimmed.to_string(), port)
}

// ---------------------------------------------------------------------------
// Subscription state (sidecar)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubscriptionsState {
    /// Signature of the last injected set — so we do not restart without need.
    pub signature: Option<String>,
    pub entries: HashMap<String, SubStateEntry>,
    /// Tags of groups that subscriptions *created* (not merely filled). On the
    /// next update these are removed before re-injecting, so a removed/disabled
    /// subscription's groups disappear. Groups the user owns and we only filled
    /// are not listed here and are left in place.
    pub created_groups: Vec<String>,
    /// True when subscription settings were saved but have not yet been
    /// injected into the running config — drives the "Apply" button state.
    pub apply_pending: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubStateEntry {
    pub last_updated: u64,
    pub node_count: usize,
    pub last_error: Option<String>,
}

fn state_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join(STATE_FILE))
}

pub fn load_state() -> Result<SubscriptionsState> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(SubscriptionsState::default());
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
    if raw.trim().is_empty() {
        return Ok(SubscriptionsState::default());
    }
    serde_json::from_str(&raw).map_err(|e| Error::parse(path.display().to_string(), e))
}

/// Record that subscription settings changed on disk and have not yet been
/// injected into the running config. Called from `save_settings` when the
/// `subscriptions` field differs from the previous value.
pub fn mark_apply_pending() {
    let mut state = match load_state() {
        Ok(s) => s,
        Err(_) => return,
    };
    if !state.apply_pending {
        state.apply_pending = true;
        let _ = save_state(&state);
    }
}

pub fn save_state(state: &SubscriptionsState) -> Result<()> {
    let path = state_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let body = serde_json::to_string_pretty(state).unwrap_or_default();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generates a short stable identifier for a new subscription.
pub fn new_id() -> String {
    let mut buf = [0u8; 4];
    if getrandom::fill(&mut buf).is_err() {
        // Fallback: a pseudo-id from the time. Not needed in practice.
        return format!("{:x}", now_millis());
    }
    hex(&buf)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SingBoxSettings;

    fn tag(v: &Value) -> String {
        v.get("tag")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
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
        // A URI list yields only nodes — no groups, no endpoints.
        assert_eq!(out.nodes.len(), 2);
        assert_eq!(out.nodes[0]["type"], "trojan");
        assert_eq!(out.nodes[1]["type"], "vless");
        assert!(out.groups.is_empty());
        assert!(out.endpoints.is_empty());
    }

    #[test]
    fn parses_singbox_json() {
        let cfg = r#"{"outbounds":[{"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}},{"type":"selector","tag":"main","outbounds":["X"]}]}"#;
        let out = parse_content(cfg).unwrap();
        // The proxy node is kept; the selector is now a group, not filtered out.
        assert_eq!(out.nodes.len(), 1);
        assert_eq!(out.nodes[0]["type"], "trojan");
        assert_eq!(out.groups.len(), 1);
        assert_eq!(out.groups[0]["type"], "selector");
        assert_eq!(tag(&out.groups[0]), "main");
    }

    #[test]
    fn parses_singbox_json_with_endpoints() {
        let cfg = r#"{"outbounds":[{"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}],"endpoints":[{"type":"wireguard","tag":"wg","system":false}]}"#;
        let out = parse_content(cfg).unwrap();
        assert_eq!(out.nodes.len(), 1);
        assert_eq!(out.endpoints.len(), 1);
        assert_eq!(out.endpoints[0]["type"], "wireguard");
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

    fn cfg_nodes(nodes: Vec<Value>) -> ParsedConfig {
        ParsedConfig {
            nodes,
            endpoints: Vec::new(),
            groups: Vec::new(),
        }
    }

    fn tmp_config_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vantage-box-sub-test-{label}.json"))
    }

    fn mksettings(path: &str, subs: Vec<SubscriptionSettings>) -> Settings {
        Settings {
            sing_box: SingBoxSettings {
                config_path: path.to_string(),
                ..Default::default()
            },
            subscriptions: subs,
            ..Default::default()
        }
    }

    fn refs_of(o: &Value) -> Vec<String> {
        o.get("outbounds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn signature_stable_regardless_of_order() {
        // Two subscriptions, one node each. Changing the order — the signature
        // of the set must not depend on the order the subscriptions came in.
        let ob_a = vec![
            json!({"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
        ];
        let ob_b = vec![json!({"type":"vless","tag":"Y","server":"h2","server_port":1,"uuid":"u"})];
        let p1 = prepare_outbounds(&[
            (mksub("a"), cfg_nodes(ob_a.clone()), None),
            (mksub("b"), cfg_nodes(ob_b.clone()), None),
        ]);
        let p2 = prepare_outbounds(&[
            (mksub("b"), cfg_nodes(ob_b), None),
            (mksub("a"), cfg_nodes(ob_a), None),
        ]);
        assert_eq!(signature(&p1), signature(&p2));
    }

    #[test]
    fn signature_detects_change() {
        let ob = vec![
            json!({"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
        ];
        let p = prepare_outbounds(&[(mksub("a"), cfg_nodes(ob), None)]);
        let ob2 = vec![json!({"type":"vless","tag":"X","server":"h","server_port":1,"uuid":"u"})];
        let p2 = prepare_outbounds(&[(mksub("a"), cfg_nodes(ob2), None)]);
        assert_ne!(signature(&p), signature(&p2));
    }

    #[test]
    fn signature_detects_group_change() {
        // Same nodes, but one has a group and the other does not — the signature
        // must differ, otherwise a group-only change would not trigger re-inject.
        let node = vec![
            json!({"type":"trojan","tag":"X","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
        ];
        let with_group = ParsedConfig {
            nodes: node.clone(),
            endpoints: Vec::new(),
            groups: vec![json!({"type":"selector","tag":"g","outbounds":["X"]})],
        };
        let p = prepare_outbounds(&[(mksub("a"), cfg_nodes(node), None)]);
        let p2 = prepare_outbounds(&[(mksub("a"), with_group, None)]);
        assert_ne!(signature(&p), signature(&p2));
    }

    #[test]
    fn prepare_outbounds_tags_with_prefix() {
        let ob = vec![
            json!({"type":"trojan","tag":"MyNode","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
        ];
        let prepared = prepare_outbounds(&[(mksub("a"), cfg_nodes(ob), None)]);
        let (_, s) = &prepared[0];
        assert_eq!(s.nodes.len(), 1);
        let t = tag(&s.nodes[0]);
        assert!(t.starts_with("sub:a:"));
        assert!(t.ends_with("MyNode"));
        assert_eq!(s.nodes[0]["tag"], t);
    }

    #[test]
    fn prepare_rewrites_group_refs() {
        // A subscription with a node, a wireguard endpoint, and two groups:
        // an auto urltest referencing the node + endpoint, and a manual selector
        // referencing the auto group + node + endpoint.
        let cfg = ParsedConfig {
            nodes: vec![
                json!({"type":"trojan","tag":"A","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
            ],
            endpoints: vec![json!({"type":"wireguard","tag":"wg","system":false})],
            groups: vec![
                json!({"type":"urltest","tag":"auto","outbounds":["A","wg"]}),
                json!({"type":"selector","tag":"manual","outbounds":["auto","A","wg"]}),
            ],
        };
        let prepared = prepare_outbounds(&[(mksub("s"), cfg, None)]);
        let (_, s) = &prepared[0];

        let a_tag = tag(&s.nodes[0]);
        let wg_tag = tag(&s.endpoints[0]);
        assert!(a_tag.starts_with("sub:s:"));
        assert!(wg_tag.starts_with("sub:s:wg"));

        // Group tags are left unchanged.
        assert_eq!(tag(&s.groups[0]), "auto");
        assert_eq!(tag(&s.groups[1]), "manual");

        // auto: node + endpoint refs rewritten to the retagged tags.
        assert_eq!(refs_of(&s.groups[0]), vec![a_tag.clone(), wg_tag.clone()]);
        // manual: the nested "auto" group ref stays; node + endpoint refs rewritten.
        assert_eq!(
            refs_of(&s.groups[1]),
            vec!["auto".to_string(), a_tag, wg_tag]
        );
    }

    #[test]
    fn inject_creates_new_groups_and_fills_existing() {
        let path = tmp_config_path("create_fill");
        // Base config: a "proxy" selector and a pre-created empty "manual".
        std::fs::write(
            &path,
            r#"{"outbounds":[
            {"type":"direct","tag":"direct"},
            {"type":"selector","tag":"proxy","outbounds":["direct"]},
            {"type":"selector","tag":"manual","outbounds":[]}
        ]}"#,
        )
        .unwrap();

        let cfg = ParsedConfig {
            nodes: vec![
                json!({"type":"trojan","tag":"A","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
            ],
            endpoints: vec![json!({"type":"wireguard","tag":"wg","system":false})],
            groups: vec![
                json!({"type":"urltest","tag":"auto","outbounds":["A","wg"]}),
                json!({"type":"selector","tag":"manual","outbounds":["auto","A","wg"]}),
            ],
        };
        let prepared = prepare_outbounds(&[(mksub("s"), cfg, None)]);
        let settings = mksettings(path.to_str().unwrap(), vec![mksub("s")]);
        let (_content, created) = inject_into_config(&settings, &prepared, &[]).unwrap();

        // "auto" is created; "manual" pre-existed so it is filled, not created.
        assert!(created.iter().any(|t| t == "auto"));
        assert!(!created.iter().any(|t| t == "manual"));

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let obs = written["outbounds"].as_array().unwrap();
        let a_tag = "sub:s:A".to_string();
        let wg_tag = "sub:s:wg".to_string();

        let auto = obs.iter().find(|o| tag(o) == "auto").unwrap();
        assert_eq!(auto["type"], "urltest");
        assert_eq!(refs_of(auto), vec![a_tag.clone(), wg_tag.clone()]);

        let manual = obs.iter().find(|o| tag(o) == "manual").unwrap();
        assert_eq!(refs_of(manual), vec!["auto".to_string(), a_tag, wg_tag]);

        // The wireguard endpoint is added under `endpoints`, retagged.
        let eps = written["endpoints"].as_array().unwrap();
        assert_eq!(eps.len(), 1);
        assert_eq!(tag(&eps[0]), "sub:s:wg");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inject_is_idempotent() {
        let path = tmp_config_path("idempotent");
        std::fs::write(
            &path,
            r#"{"outbounds":[
                {"type":"direct","tag":"direct"},
                {"type":"selector","tag":"proxy","outbounds":["direct"]}
            ]}"#,
        )
        .unwrap();

        let cfg = ParsedConfig {
            nodes: vec![
                json!({"type":"trojan","tag":"A","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
            ],
            endpoints: vec![json!({"type":"wireguard","tag":"wg","system":false})],
            groups: vec![
                json!({"type":"urltest","tag":"auto","outbounds":["A","wg"]}),
                json!({"type":"selector","tag":"manual","outbounds":["auto","A","wg"]}),
            ],
        };
        let prepared = prepare_outbounds(&[(mksub("s"), cfg, None)]);
        let settings = mksettings(path.to_str().unwrap(), vec![mksub("s")]);

        let (_, created1) = inject_into_config(&settings, &prepared, &[]).unwrap();
        // Second pass feeds back the groups we created, as `apply` would persist.
        let (_, created2) = inject_into_config(&settings, &prepared, &created1).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let obs = written["outbounds"].as_array().unwrap();

        // Still exactly one "auto" and one "manual" — no duplicates grew.
        assert_eq!(obs.iter().filter(|o| tag(o) == "auto").count(), 1);
        assert_eq!(obs.iter().filter(|o| tag(o) == "manual").count(), 1);
        // Still exactly one managed node and one managed endpoint.
        assert_eq!(obs.iter().filter(|o| tag(o).starts_with("sub:")).count(), 1);
        let manual = obs.iter().find(|o| tag(o) == "manual").unwrap();
        // manual refs do not duplicate on a second pass.
        assert_eq!(
            refs_of(manual),
            vec![
                "auto".to_string(),
                "sub:s:A".to_string(),
                "sub:s:wg".to_string()
            ]
        );
        // The created set is stable across passes.
        assert_eq!(created1, created2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inject_removes_created_groups_when_gone() {
        let path = tmp_config_path("remove");
        // Simulate a config after a previous inject: a created "auto" group,
        // a filled "manual", a managed node, and a managed endpoint.
        std::fs::write(
            &path,
            r#"{"outbounds":[
                {"type":"direct","tag":"direct"},
                {"type":"selector","tag":"proxy","outbounds":["direct"]},
                {"type":"selector","tag":"manual","outbounds":["auto"]},
                {"type":"urltest","tag":"auto","outbounds":["sub:s:A"]},
                {"type":"trojan","tag":"sub:s:A","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}
            ],"endpoints":[{"type":"wireguard","tag":"sub:s:wg","system":false}]}"#,
        )
        .unwrap();

        // No subscriptions anymore — nothing to inject. `prev_created` says we
        // created "auto" last time, so it must be removed.
        let settings = mksettings(path.to_str().unwrap(), vec![]);
        let prepared: Prepared = Vec::new();
        let (_content, created) =
            inject_into_config(&settings, &prepared, &["auto".to_string()]).unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let obs = written["outbounds"].as_array().unwrap();

        // The created "auto" group is gone; the user-owned "manual" and "proxy" stay.
        assert!(obs.iter().all(|o| tag(o) != "auto"));
        assert!(obs.iter().any(|o| tag(o) == "manual"));
        assert!(obs.iter().any(|o| tag(o) == "proxy"));
        // Managed nodes are removed.
        assert!(obs.iter().all(|o| !tag(o).starts_with("sub:")));
        // "manual" had only a sub: ref ("auto" was a group ref actually — wait, the
        // manual outbounds was ["auto"]; "auto" is now gone but the ref "auto" is
        // not sub:-prefixed, so it stays. Stripping only removes sub: refs.)
        let manual = obs.iter().find(|o| tag(o) == "manual").unwrap();
        assert_eq!(refs_of(manual), vec!["auto".to_string()]);
        // Managed endpoint removed.
        let eps = written["endpoints"].as_array().unwrap();
        assert!(eps.is_empty());
        assert!(created.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inject_node_only_pours_into_all_groups() {
        // A URI-list-style subscription (nodes only, no groups) still pours into
        // all existing selector/urltest groups — the legacy behavior.
        let path = tmp_config_path("node_only");
        std::fs::write(
            &path,
            r#"{"outbounds":[
                {"type":"direct","tag":"direct"},
                {"type":"selector","tag":"proxy","outbounds":["direct"]},
                {"type":"urltest","tag":"auto","outbounds":["direct"]}
            ]}"#,
        )
        .unwrap();
        let cfg = cfg_nodes(vec![
            json!({"type":"trojan","tag":"A","server":"h","server_port":1,"password":"p","tls":{"enabled":true}}),
        ]);
        let prepared = prepare_outbounds(&[(mksub("s"), cfg, None)]);
        let settings = mksettings(path.to_str().unwrap(), vec![mksub("s")]);
        let (_content, created) = inject_into_config(&settings, &prepared, &[]).unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let obs = written["outbounds"].as_array().unwrap();
        let a_tag = "sub:s:A".to_string();
        let proxy = obs.iter().find(|o| tag(o) == "proxy").unwrap();
        assert!(refs_of(proxy).contains(&a_tag));
        let auto = obs.iter().find(|o| tag(o) == "auto").unwrap();
        assert!(refs_of(auto).contains(&a_tag));
        // No groups created by a node-only subscription.
        assert!(created.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
