//! Application windows: the main window and the proxy-selection popup.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";
pub const POPUP: &str = "popup";

/// The application is quitting for real, not minimizing to the tray.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// The popup has received focus at least once since it was shown.
static POPUP_HAD_FOCUS: AtomicBool = AtomicBool::new(false);

/// When the popup was last shown.
static POPUP_SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// While a window is being shown, focus can jump back and forth between it and
/// whatever was active before. Within this window of time we ignore focus
/// loss, otherwise the popup would close right at the moment of opening.
const FOCUS_GRACE: Duration = Duration::from_millis(600);

fn mark_shown() {
    *POPUP_SHOWN_AT.lock().expect("popup shown lock") = Some(Instant::now());
    POPUP_HAD_FOCUS.store(false, Ordering::SeqCst);
}

fn within_grace() -> bool {
    POPUP_SHOWN_AT
        .lock()
        .expect("popup shown lock")
        .is_some_and(|shown| shown.elapsed() < FOCUS_GRACE)
}

/// Full quit. Without this flag the window-close handler would treat a quit
/// as an attempt to minimize the app and cancel it.
pub fn quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    // sing-box started by us as a process belongs to the application: it must
    // not survive the quit, otherwise it would keep running with no way to
    // manage it. The service, on the other hand, lives on its own and stays.
    if let Err(e) = crate::process::stop() {
        eprintln!("failed to stop sing-box on quit: {e}");
    }
    app.exit(0);
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Popup size in logical pixels. A node row is 20px, so this is about two
/// dozen nodes without scrolling.
const POPUP_WIDTH: f64 = 320.0;
const POPUP_HEIGHT: f64 = 420.0;
/// Popup margin from the screen edge and from the cursor.
const POPUP_MARGIN: f64 = 12.0;

/// Shows the main window and brings it to the front. If it was minimized to
/// the tray — restores it.
pub fn show_main(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Hides the main window into the tray.
pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

/// Shows the popup at the cursor; if it is already open — closes it.
/// That is exactly how a hotkey invocation behaves: the second press dismisses
/// the window.
pub fn toggle_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(POPUP) {
        if window.is_visible().unwrap_or(false) {
            hide_popup(app);
        } else {
            present(app, &window);
        }
        return;
    }

    match build_popup(app) {
        Ok(window) => present(app, &window),
        Err(e) => eprintln!("failed to open the popup: {e}"),
    }
}

/// Showing the popup: position, timestamp for the grace period, show, focus.
/// Both opening paths must go through it — a forgotten `mark_shown` means the
/// window will close itself the moment it appears.
fn present(app: &AppHandle, window: &tauri::WebviewWindow) {
    position_at_cursor(app, window);
    mark_shown();
    if let Err(e) = window.show() {
        eprintln!("failed to show the popup: {e}");
    }
    let _ = window.set_focus();
}

pub fn hide_popup(app: &AppHandle) {
    POPUP_HAD_FOCUS.store(false, Ordering::SeqCst);
    *POPUP_SHOWN_AT.lock().expect("popup shown lock") = None;
    if let Some(window) = app.get_webview_window(POPUP) {
        let _ = window.hide();
    }
}

/// The popup is the same app, but with a different view: the frontend looks at
/// the window label and draws a compact group list instead of the full UI.
///
/// The address must be the app root. Any path or query string would send the
/// SvelteKit router to a non-existent route, and instead of the popup a 404
/// page would open — the window is there, but the contents are not.
fn build_popup(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, POPUP, WebviewUrl::App("index.html".into()))
        .title("Vantage Box")
        .inner_size(POPUP_WIDTH, POPUP_HEIGHT)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(true)
        // Show only after positioning, otherwise the window flashes in the corner.
        .visible(false)
        .build()?;

    // The popup lives as long as it has focus: a click outside — and it is gone.
    //
    // Hide only if focus was actually had and then lost. Right after show()
    // the window can receive Focused(false) without ever having been active —
    // without this check the popup would close the moment it opened.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(focused) = event {
            if *focused {
                POPUP_HAD_FOCUS.store(true, Ordering::SeqCst);
            } else if !within_grace() && POPUP_HAD_FOCUS.swap(false, Ordering::SeqCst) {
                hide_popup(&handle);
            }
        }
    });

    Ok(window)
}

/// Places the window next to the cursor, keeping it from going off the screen edge.
fn position_at_cursor(app: &AppHandle, window: &tauri::WebviewWindow) {
    let Ok(cursor) = app.cursor_position() else {
        return;
    };

    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        let _ = window.set_position(tauri::PhysicalPosition::new(cursor.x, cursor.y));
        return;
    };

    // Compute in logical coordinates: on scaled displays physical pixels do
    // not match the sizes given to the window.
    let scale = monitor.scale_factor();
    let area_position: LogicalPosition<f64> = monitor.position().to_logical(scale);
    let area_size: LogicalSize<f64> = monitor.size().to_logical(scale);
    let cursor = LogicalPosition::new(cursor.x / scale, cursor.y / scale);

    let max_x = area_position.x + area_size.width - POPUP_WIDTH - POPUP_MARGIN;
    let max_y = area_position.y + area_size.height - POPUP_HEIGHT - POPUP_MARGIN;

    let x = (cursor.x - POPUP_WIDTH / 2.0)
        .clamp(area_position.x + POPUP_MARGIN, max_x.max(area_position.x));
    let y = (cursor.y - POPUP_HEIGHT - POPUP_MARGIN)
        .clamp(area_position.y + POPUP_MARGIN, max_y.max(area_position.y));

    let _ = window.set_position(LogicalPosition::new(x, y));
}
