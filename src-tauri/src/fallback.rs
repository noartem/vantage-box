//! Auto-switching selector groups to a backup when the active node fails.
//!
//! We periodically ping the active node of each watched selector group. If it
//! did not respond or the delay exceeded `max_delay_ms`, we measure the whole
//! group and switch to the node with the lowest valid delay. We do not touch
//! `urltest` groups — they manage the selection themselves.
//!
//! The source of settings is `settings.fallback`; the latency-test URL and
//! timeout come from `settings.ui`. If fallback is off, the task simply sleeps
//! and re-checks the setting once a minute, so enabling it is picked up without
//! a restart.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::actions::EVENT_PROXIES;
use crate::state::AppState;

/// Polling interval for the setting when fallback is off.
const IDLE_SLEEP: u64 = 60;
/// The minimum real interval — so a bad setting does not hammer the API.
const MIN_INTERVAL: u32 = 5;

/// Background task: started once at application startup.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sleep = tick(&app).await;
            tokio::time::sleep(Duration::from_secs(sleep)).await;
        }
    });
}

/// One check pass. Returns how many seconds to wait before the next one.
async fn tick(app: &AppHandle) -> u64 {
    let settings = app.state::<AppState>().settings.get();
    let fb = settings.fallback.clone();
    if !fb.enabled {
        return IDLE_SLEEP;
    }
    let interval = fb.interval_sec.max(MIN_INTERVAL) as u64;

    let client = app.state::<AppState>().client();
    let url = settings.ui.latency_test_url.clone();
    let timeout = fb.timeout_ms;
    let max_delay = fb.max_delay_ms;

    let proxies = match client.proxies().await {
        Ok(r) => r.proxies,
        Err(_) => {
            // sing-box is not running or the Clash API is unavailable — nothing to ping.
            return interval;
        }
    };

    // Which groups we watch: an explicit list from settings, otherwise all selectors.
    let groups: Vec<String> = if fb.groups.is_empty() {
        proxies
            .iter()
            .filter(|(_, p)| p.is_selectable())
            .map(|(name, _)| name.clone())
            .collect()
    } else {
        fb.groups.clone()
    };

    for group in groups {
        let Some(g) = proxies.get(&group) else { continue };
        // urltest manages itself — we do not interfere. And "select" is
        // meaningless for it.
        if !g.is_selectable() {
            continue;
        }
        let Some(active) = g.now.as_ref() else { continue };
        let members = g.all.clone().unwrap_or_default();

        if is_ok(&client, active, &url, timeout, max_delay).await {
            continue;
        }

        // The active one is bad — measure the whole group and pick the best backup.
        let delays = match client.group_delay(&group, &url, timeout).await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("fallback: measuring {group} failed: {e}");
                continue;
            }
        };
        let best = delays
            .iter()
            .filter(|(name, _)| name.as_str() != active.as_str())
            .filter(|(_, d)| **d > 0 && (max_delay == 0 || **d <= max_delay))
            .min_by_key(|(_, d)| *d)
            .map(|(name, _)| name.clone());

        if let Some(best) = best {
            if let Err(e) = client.select(&group, &best).await {
                eprintln!("fallback: failed to switch {group}: {e}");
                continue;
            }
            let _ = app.emit(EVENT_PROXIES, ());
            eprintln!("fallback: {group} → {best} (instead of {active})");
        }
        // If no live backup exists — keep the active one: switching to a node
        // known to be dead is pointless.
        let _ = &members;
    }

    interval
}

/// Whether the node reached an acceptable delay: responded (>0) and within
/// `max_delay` (0 — no limit set, any responding delay is fine).
async fn is_ok(
    client: &crate::clash::ClashClient,
    name: &str,
    url: &str,
    timeout: u32,
    max_delay: u32,
) -> bool {
    match client.proxy_delay(name, url, timeout).await {
        Ok(delay) => delay > 0 && (max_delay == 0 || delay <= max_delay),
        Err(_) => false,
    }
}