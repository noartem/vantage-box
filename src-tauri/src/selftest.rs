//! Self-check via the `--self-test` flag.
//!
//! Part of M2 cannot be verified from the outside: the popup is a separate
//! window opened by a global hotkey, and if its webview did not load, that is
//! only visible to the eye. Here the app opens the popup itself and waits for
//! a readiness signal from it — i.e. it exercises the whole path end to end:
//! window creation, frontend load, and its startup.
//!
//! The result is printed as a single line, read by `scripts/smoke-test.ps1`.

use std::time::Duration;

use tauri::{AppHandle, Listener, Manager};

/// The command-line flag that enables the self-check.
pub const FLAG: &str = "--self-test";

/// The event the popup sends from `onMount`.
const EVENT_POPUP_READY: &str = "popup://ready";

/// How long we wait for the popup webview to load.
const POPUP_TIMEOUT: Duration = Duration::from_secs(15);

pub fn requested() -> bool {
    std::env::args().any(|arg| arg == FLAG)
}

/// Runs the checks and exits the application with the appropriate code.
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

    // Subscribe before opening the window: the popup may load faster than we
    // get back to subscribing.
    app.listen(EVENT_POPUP_READY, move |_| {
        if let Some(tx) = tx.lock().expect("selftest channel lock").take() {
            let _ = tx.send(());
        }
    });

    crate::window::toggle_popup(app);

    let ready = tokio::time::timeout(POPUP_TIMEOUT, rx).await.is_ok();
    if !ready {
        // The window URL is the first thing worth seeing on such a failure: most
        // often the webview simply loaded the wrong thing.
        let url = app
            .get_webview_window(crate::window::POPUP)
            .and_then(|w| w.url().ok())
            .map(|u| u.to_string())
            .unwrap_or_else(|| "<no window>".into());
        eprintln!("vantage-box selftest: popup did not report readiness, url={url}");
        return false;
    }

    // Let the window settle: show and focus acquisition are asynchronous.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let visible = app
        .get_webview_window(crate::window::POPUP)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if !visible {
        eprintln!("vantage-box selftest: popup window created but not shown");
    }

    crate::window::hide_popup(app);
    visible
}