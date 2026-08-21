//! The flat `/proxies` response is inconvenient for clients, so we break it
//! down into groups with nodes already filled in and the latest latency.
//!
//! Shared between the Tauri command (`commands::get_proxies`) and the IPC bus
//! (`ipc::handlers`), so the GUI and external integrations see the same shape.

use std::collections::HashMap;

use serde::Serialize;

use super::models::Proxy;

/// A group-level view of `/proxies`: groups with their nodes and latency.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyOverview {
    pub groups: Vec<GroupView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub name: String,
    /// `Selector`, `URLTest`, …
    pub kind: String,
    pub now: Option<String>,
    /// Whether the selection can be changed by hand.
    pub selectable: bool,
    pub items: Vec<NodeView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub name: String,
    pub kind: String,
    /// Latest known measurement, ms. `None` — not measured or the node did not respond.
    pub delay: Option<u32>,
    pub udp: bool,
    /// Nested group: clicking it should go inside, not just select.
    pub is_group: bool,
}

/// Builds the group-oriented view from the raw `/proxies` map.
///
/// The source is a `HashMap`, so the order would drift from call to call.
/// We take the group order from `GLOBAL` when sing-box returns it — it reflects
/// the outbound order in the config — and fall back to alphabetical.
pub fn build_overview(proxies: HashMap<String, Proxy>) -> ProxyOverview {
    // Take the group order from GLOBAL, if sing-box returned it: it reflects
    // the outbound order in the config, not a random hash-table traversal.
    let global_order: Vec<String> = proxies
        .get("GLOBAL")
        .and_then(|p| p.all.clone())
        .unwrap_or_default();

    // Take the name from the map key: sing-box does not always return the
    // `name` field inside the object, but the key is always there.
    let mut groups: Vec<GroupView> = proxies
        .iter()
        .filter(|(name, p)| p.is_group() && name.as_str() != "GLOBAL")
        .map(|(group_name, group)| {
            let items = group
                .all
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|name| {
                    let node = proxies.get(&name);
                    NodeView {
                        delay: node
                            .and_then(|n| n.history.last())
                            .map(|h| h.delay)
                            // Zero means "the node did not respond", not an instant reply.
                            .filter(|d| *d > 0),
                        kind: node.map(|n| n.kind.clone()).unwrap_or_default(),
                        udp: node.is_some_and(|n| n.udp),
                        is_group: node.is_some_and(|n| n.is_group()),
                        name,
                    }
                })
                .collect();

            GroupView {
                name: group_name.clone(),
                kind: group.kind.clone(),
                now: group.now.clone(),
                selectable: group.is_selectable(),
                items,
            }
        })
        .collect();

    // Order from GLOBAL, everything else alphabetically after it. Without the
    // second key the order would drift from call to call: the source is a HashMap.
    groups.sort_by_key(|g| {
        let rank = global_order
            .iter()
            .position(|n| n == &g.name)
            .unwrap_or(usize::MAX);
        (rank, g.name.clone())
    });

    ProxyOverview { groups }
}
