//! Автопереключение selector-групп на резерв при отказе активного узла.
//!
//! Периодически пингуем активный узел каждой отслеживаемой selector-группы.
//! Если он не ответил или задержка превысила `max_delay_ms`, замеряем всю
//! группу и переключаем на узел с наименьшей валидной задержкой. `urltest`-
//! группы не трогаем — они рушат выбор сами.
//!
//! Источник настроек — `settings.fallback`; URL и таймаут latency-теста —
//! из `settings.ui`. Если fallback выключен, задача просто спит и раз в
//! минуту перепроверяет настройку, чтобы включение подхватилось без
//! перезапуска.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::actions::EVENT_PROXIES;
use crate::state::AppState;

/// Интервал опроса настройки, когда fallback выключен.
const IDLE_SLEEP: u64 = 60;
/// Минимальный реальный интервал — чтобы при кривой настройке не молотить API.
const MIN_INTERVAL: u32 = 5;

/// Фоновая задача: запускается один раз при старте приложения.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sleep = tick(&app).await;
            tokio::time::sleep(Duration::from_secs(sleep)).await;
        }
    });
}

/// Один проход проверки. Возвращает, сколько секунд ждать до следующего.
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
            // sing-box не запущен или Clash API недоступен — нечего пинговать.
            return interval;
        }
    };

    // Какие группы контролируем: явный список из настроек, иначе все selector'ы.
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
        // urltest рулит сам — туда не лезем. Да и «select» для него бессмысленен.
        if !g.is_selectable() {
            continue;
        }
        let Some(active) = g.now.as_ref() else { continue };
        let members = g.all.clone().unwrap_or_default();

        if is_ok(&client, active, &url, timeout, max_delay).await {
            continue;
        }

        // Активный плох — замеряем всю группу и выбираем лучшего резервного.
        let delays = match client.group_delay(&group, &url, timeout).await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("fallback: замер {group} не удался: {e}");
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
                eprintln!("fallback: не удалось переключить {group}: {e}");
                continue;
            }
            let _ = app.emit(EVENT_PROXIES, ());
            eprintln!("fallback: {group} → {best} (вместо {active})");
        }
        // Если ни одного живого резервного нет — оставляем активный: переключаться
        // на заведомо мёртвый узел бессмысленно.
        let _ = &members;
    }

    interval
}

/// Достиг ли узел приемлемой задержки: ответил (>0) и в пределах `max_delay`
/// (0 — ограничение не задано, любая ответившая задержка годится).
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