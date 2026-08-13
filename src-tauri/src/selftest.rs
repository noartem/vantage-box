//! Самопроверка по флагу `--self-test`.
//!
//! Часть M2 нельзя проверить снаружи: попап — отдельное окно, которое
//! открывается по глобальному хоткею, и если его webview не загрузился, это
//! видно только глазами. Здесь приложение открывает попап само и дожидается
//! от него сигнала о готовности — то есть проверяет весь путь целиком:
//! создание окна, загрузку фронтенда и его запуск.
//!
//! Результат печатается одной строкой, её читает `scripts/smoke-test.ps1`.

use std::time::Duration;

use tauri::{AppHandle, Listener, Manager};

/// Флаг командной строки, включающий самопроверку.
pub const FLAG: &str = "--self-test";

/// Событие, которое попап шлёт из `onMount`.
const EVENT_POPUP_READY: &str = "popup://ready";

/// Сколько ждём загрузки webview попапа.
const POPUP_TIMEOUT: Duration = Duration::from_secs(15);

pub fn requested() -> bool {
    std::env::args().any(|arg| arg == FLAG)
}

/// Запускает проверки и завершает приложение с соответствующим кодом.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let popup_ok = check_popup(&app).await;
        eprintln!(
            "vantage-box selftest popup={}",
            if popup_ok { "ok" } else { "failed" }
        );
        crate::window::quit(&app);
    });
}

async fn check_popup(app: &AppHandle) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = std::sync::Mutex::new(Some(tx));

    // Подписываемся до открытия окна: попап успевает загрузиться быстрее,
    // чем мы вернулись бы к подписке.
    app.listen(EVENT_POPUP_READY, move |_| {
        if let Some(tx) = tx.lock().expect("selftest channel lock").take() {
            let _ = tx.send(());
        }
    });

    crate::window::toggle_popup(app);

    let ready = tokio::time::timeout(POPUP_TIMEOUT, rx).await.is_ok();
    if !ready {
        // Адрес окна — первое, что стоит увидеть при таком отказе: чаще всего
        // webview просто загрузил не то.
        let url = app
            .get_webview_window(crate::window::POPUP)
            .and_then(|w| w.url().ok())
            .map(|u| u.to_string())
            .unwrap_or_else(|| "<окна нет>".into());
        eprintln!("vantage-box selftest: попап не сообщил о готовности, url={url}");
        return false;
    }

    // Даём окну устояться: показ и получение фокуса асинхронны.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let visible = app
        .get_webview_window(crate::window::POPUP)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if !visible {
        eprintln!("vantage-box selftest: окно попапа создано, но не показано");
    }

    crate::window::hide_popup(app);
    visible
}
