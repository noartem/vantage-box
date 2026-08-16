//! Окна приложения: главное и всплывающий выбор прокси.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";
pub const POPUP: &str = "popup";

/// Приложение закрывается по-настоящему, а не сворачивается в трей.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Попап хотя бы раз получал фокус с момента показа.
static POPUP_HAD_FOCUS: AtomicBool = AtomicBool::new(false);

/// Когда попап показали в последний раз.
static POPUP_SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Пока идёт показ окна, фокус успевает несколько раз перескочить между ним и
/// тем, что было активно раньше. В этом окне времени потерю фокуса игнорируем,
/// иначе попап закрывался бы прямо в момент открытия.
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

/// Полный выход. Без этого флага обработчик закрытия окна принял бы выход
/// за попытку свернуть приложение и отменил бы его.
pub fn quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    // sing-box, запущенный нами процессом, принадлежит приложению: пережить
    // выход он не должен, иначе останется висеть без единого способа им
    // управлять. Сервис — наоборот, живёт своей жизнью и остаётся работать.
    if let Err(e) = crate::process::stop() {
        eprintln!("не удалось остановить sing-box при выходе: {e}");
    }
    app.exit(0);
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Размер попапа в логических пикселях. Строка узла — 20px, так что по высоте
/// это примерно два десятка узлов без прокрутки.
const POPUP_WIDTH: f64 = 320.0;
const POPUP_HEIGHT: f64 = 420.0;
/// Отступ попапа от края экрана и от курсора.
const POPUP_MARGIN: f64 = 12.0;

/// Показывает главное окно и поднимает его наверх. Если оно было свёрнуто
/// в трей — разворачивает.
pub fn show_main(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Прячет главное окно в трей.
pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

/// Показывает попап у курсора, а если он уже открыт — закрывает.
/// Именно так ведёт себя вызов по хоткею: второе нажатие убирает окно.
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
        Err(e) => eprintln!("не удалось открыть попап: {e}"),
    }
}

/// Показ попапа: позиция, отметка времени для grace-периода, показ, фокус.
/// Оба пути открытия обязаны идти через неё — забытый `mark_shown` означает,
/// что окно закроется само в момент появления.
fn present(app: &AppHandle, window: &tauri::WebviewWindow) {
    position_at_cursor(app, window);
    mark_shown();
    if let Err(e) = window.show() {
        eprintln!("не удалось показать попап: {e}");
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

/// Попап — то же приложение, но с другим представлением: фронтенд смотрит на
/// метку окна и рисует компактный список групп вместо полного интерфейса.
///
/// Адрес обязан быть корнем приложения. Любой путь или query-строка увели бы
/// роутер SvelteKit на несуществующий маршрут, и вместо попапа открылась бы
/// страница 404 — окно есть, а содержимого нет.
fn build_popup(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, POPUP, WebviewUrl::App("index.html".into()))
    .title("Vantage Box")
    .inner_size(POPUP_WIDTH, POPUP_HEIGHT)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .shadow(true)
    // Показываем только после позиционирования, иначе окно мигнёт в углу.
    .visible(false)
    .build()?;

    // Попап живёт, пока на нём фокус: клик мимо — и он не мешает.
    //
    // Прячем только если фокус реально был и потерялся. Сразу после show()
    // окно успевает получить Focused(false), ещё ни разу не будучи активным, —
    // без этой проверки попап закрывался бы в момент открытия.
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

/// Ставит окно рядом с курсором, не давая ему уехать за край экрана.
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

    // Считаем в логических координатах: на экранах с масштабированием
    // физические пиксели не совпадают с размерами, заданными окну.
    let scale = monitor.scale_factor();
    let area_position: LogicalPosition<f64> = monitor.position().to_logical(scale);
    let area_size: LogicalSize<f64> = monitor.size().to_logical(scale);
    let cursor = LogicalPosition::new(cursor.x / scale, cursor.y / scale);

    let max_x = area_position.x + area_size.width - POPUP_WIDTH - POPUP_MARGIN;
    let max_y = area_position.y + area_size.height - POPUP_HEIGHT - POPUP_MARGIN;

    let x = (cursor.x - POPUP_WIDTH / 2.0).clamp(area_position.x + POPUP_MARGIN, max_x.max(area_position.x));
    let y = (cursor.y - POPUP_HEIGHT - POPUP_MARGIN).clamp(area_position.y + POPUP_MARGIN, max_y.max(area_position.y));

    let _ = window.set_position(LogicalPosition::new(x, y));
}
