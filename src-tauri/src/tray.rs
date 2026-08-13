//! Иконка в трее: состояние одним взглядом и управление без открытия окна.
//!
//! Меню собирается заново только когда изменилось его содержимое — иначе
//! фоновое обновление раз в несколько секунд заставляло бы меню моргать.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::actions;
use crate::clash::models::ConnectionState;
use crate::service::ServiceState;
use crate::state::AppState;
use crate::window;

const TRAY_ID: &str = "vantage-box";

/// Как часто подтягиваем состояние в трей.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// Сколько узлов показываем в подменю группы. Меню в трее на сотню элементов
/// нечитаемо, а длинные списки — это работа для окна.
const MAX_NODES_PER_GROUP: usize = 24;

// Идентификаторы пунктов меню.
const ID_TOGGLE: &str = "toggle";
const ID_RESTART: &str = "restart";
const ID_SHOW: &str = "show";
const ID_QUIT: &str = "quit";
/// Пункты выбора прокси нумеруем: теги sing-box могут содержать что угодно,
/// включая разделители, поэтому имя группы в id не кодируем.
const ID_SELECT_PREFIX: &str = "select:";

/// Что трей показывает сейчас. Нужно, чтобы не пересобирать меню впустую.
#[derive(Default)]
pub struct TrayRegistry {
    /// id пункта → (группа, узел).
    selections: Mutex<HashMap<String, (String, String)>>,
    /// Отпечаток последнего собранного меню.
    signature: Mutex<Option<String>>,
}

/// Создаёт иконку в трее. Меню наполняется первым же `refresh`.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    // Повторный вызов управляемого состояния паникует, поэтому setup
    // предполагается однократным — он и вызывается один раз при старте.
    app.manage(TrayRegistry::default());

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_for(false))
        .tooltip("Vantage Box")
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            // Левый клик открывает окно, правый — меню (его рисует система).
            if let TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                window::show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Фоновое обновление трея. Живёт столько же, сколько приложение.
pub fn spawn_refresher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app).await;
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });
}

/// Пересобирает иконку, подсказку и меню под текущее состояние.
pub async fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let status = actions::run_status(app).await.ok();
    let run = RunSummary::from(status.as_ref());

    let connection = app.state::<AppState>().streams.status();
    let connected = connection.state == ConnectionState::Connected;

    // Группы спрашиваем только при живом API: иначе это гарантированный таймаут
    // каждые несколько секунд.
    let groups = if connected {
        app.state::<AppState>()
            .client()
            .proxies()
            .await
            .ok()
            .map(|response| collect_groups(response.proxies))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let _ = tray.set_icon(Some(icon_for(run.running && connected)));
    let _ = tray.set_tooltip(Some(tooltip(run, connected, &groups)));

    let signature = signature(run, connected, &groups);
    {
        let registry = app.state::<TrayRegistry>();
        let mut last = registry.signature.lock().expect("tray signature lock");
        if last.as_deref() == Some(signature.as_str()) {
            return;
        }
        *last = Some(signature);
    }

    if let Err(e) = rebuild_menu(app, &tray, run, connected, &groups) {
        eprintln!("не удалось обновить меню трея: {e}");
    }
}

// ---------------------------------------------------------------------------
// Меню
// ---------------------------------------------------------------------------

/// Группа selector'ов в том виде, в котором её показывает трей.
struct TrayGroup {
    name: String,
    now: Option<String>,
    nodes: Vec<String>,
    /// Сколько узлов не поместилось.
    hidden: usize,
}

fn collect_groups(
    proxies: HashMap<String, crate::clash::models::Proxy>,
) -> Vec<TrayGroup> {
    let mut groups: Vec<TrayGroup> = proxies
        .iter()
        .filter(|(name, proxy)| {
            proxy.is_group() && proxy.is_selectable() && name.as_str() != "GLOBAL"
        })
        .map(|(name, proxy)| {
            let all = proxy.all.clone().unwrap_or_default();
            let hidden = all.len().saturating_sub(MAX_NODES_PER_GROUP);
            TrayGroup {
                name: name.clone(),
                now: proxy.now.clone(),
                nodes: all.into_iter().take(MAX_NODES_PER_GROUP).collect(),
                hidden,
            }
        })
        .collect();

    // Источник — HashMap, поэтому без сортировки порядок пунктов менялся бы
    // от обновления к обновлению.
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    groups
}

/// Всё, что трею нужно знать о состоянии sing-box. Копия, а не ссылка на
/// `RunStatus`: она должна оставаться `Copy`, её таскают по всем подписям.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RunSummary {
    running: bool,
    /// Состояние вообще удалось прочитать.
    known: bool,
    /// Запуск возможен: либо сервис установлен, либо конфигу не нужен TUN.
    can_start: bool,
    /// Идёт переход между состояниями — управлять сейчас нечем.
    pending: bool,
}

impl RunSummary {
    fn from(status: Option<&actions::RunStatus>) -> Self {
        let Some(status) = status else {
            return Self {
                running: false,
                known: false,
                can_start: false,
                pending: false,
            };
        };
        Self {
            running: status.running,
            known: true,
            can_start: status.can_start(),
            pending: matches!(
                status.service.state,
                ServiceState::StartPending | ServiceState::StopPending
            ),
        }
    }
}

fn rebuild_menu(
    app: &AppHandle,
    tray: &tauri::tray::TrayIcon,
    run: RunSummary,
    connected: bool,
    groups: &[TrayGroup],
) -> tauri::Result<()> {
    let registry = app.state::<TrayRegistry>();
    let mut selections = HashMap::new();

    let header = MenuItem::with_id(
        app,
        "header",
        format!("Vantage Box — {}", state_label(run, connected)),
        false,
        None::<&str>,
    )?;

    let mut menu = MenuBuilder::new(app).item(&header).separator();

    // Подменю групп. Держим их живыми до вызова build(): SubmenuBuilder
    // возвращает значения, на которые мы дальше ссылаемся.
    let mut submenus = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        let mut items: Vec<Box<dyn IsMenuItem<_>>> = Vec::new();

        for (node_index, node) in group.nodes.iter().enumerate() {
            let id = format!("{ID_SELECT_PREFIX}{group_index}:{node_index}");
            selections.insert(id.clone(), (group.name.clone(), node.clone()));
            items.push(Box::new(CheckMenuItem::with_id(
                app,
                &id,
                node,
                true,
                group.now.as_deref() == Some(node.as_str()),
                None::<&str>,
            )?));
        }

        if group.hidden > 0 {
            items.push(Box::new(MenuItem::with_id(
                app,
                format!("more:{group_index}"),
                format!("…ещё {} — откройте окно", group.hidden),
                false,
                None::<&str>,
            )?));
        }

        let refs: Vec<&dyn IsMenuItem<_>> = items.iter().map(|i| i.as_ref()).collect();
        submenus.push(
            SubmenuBuilder::new(app, group_label(group))
                .items(&refs)
                .build()?,
        );
    }

    for submenu in &submenus {
        menu = menu.item(submenu);
    }
    if !submenus.is_empty() {
        menu = menu.separator();
    }

    let toggle_text = if run.running { "Остановить" } else { "Запустить" };
    // Пока идёт переход, команда всё равно не выполнится. Запуск без сервиса
    // невозможен только для конфига с TUN — остановка доступна всегда.
    let controllable = run.known && !run.pending && (run.running || run.can_start);

    let menu = menu
        .item(&MenuItem::with_id(
            app,
            ID_TOGGLE,
            toggle_text,
            controllable,
            None::<&str>,
        )?)
        .item(&MenuItem::with_id(
            app,
            ID_RESTART,
            "Мягкий перезапуск",
            run.running && !run.pending,
            None::<&str>,
        )?)
        .separator()
        .item(&MenuItem::with_id(
            app,
            ID_SHOW,
            "Открыть Vantage Box",
            true,
            None::<&str>,
        )?)
        .separator()
        .item(&MenuItem::with_id(app, ID_QUIT, "Выход", true, None::<&str>)?)
        .build()?;

    *registry.selections.lock().expect("tray selections lock") = selections;
    tray.set_menu(Some(menu))
}

fn group_label(group: &TrayGroup) -> String {
    match &group.now {
        Some(now) => format!("{}: {now}", group.name),
        None => group.name.clone(),
    }
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().0.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let result = match id.as_str() {
            ID_TOGGLE => actions::toggle(&app).await.map(|_| ()),
            ID_RESTART => actions::restart(&app).await.map(|_| ()),
            ID_SHOW => {
                window::show_main(&app);
                Ok(())
            }
            ID_QUIT => {
                window::quit(&app);
                Ok(())
            }
            other if other.starts_with(ID_SELECT_PREFIX) => {
                let target = app
                    .state::<TrayRegistry>()
                    .selections
                    .lock()
                    .expect("tray selections lock")
                    .get(other)
                    .cloned();
                match target {
                    Some((group, node)) => actions::select_proxy(&app, &group, &node).await,
                    // Меню могло устареть между отрисовкой и кликом.
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        };

        if let Err(e) = result {
            eprintln!("действие из трея не выполнено: {e}");
        }
        refresh(&app).await;
    });
}

// ---------------------------------------------------------------------------
// Подписи и иконки
// ---------------------------------------------------------------------------

fn state_label(run: RunSummary, connected: bool) -> &'static str {
    if !run.known {
        return "состояние неизвестно";
    }
    match run {
        RunSummary { running: true, .. } if connected => "работает",
        RunSummary { running: true, .. } => "запущен, нет связи с API",
        RunSummary { pending: true, .. } => "переключается",
        RunSummary {
            can_start: false, ..
        } => "конфигу нужен TUN — установите сервис",
        _ => "остановлен",
    }
}

fn tooltip(run: RunSummary, connected: bool, groups: &[TrayGroup]) -> String {
    let mut text = format!("Vantage Box — {}", state_label(run, connected));

    // В подсказку выносим только активный outbound первой группы: тултип
    // Windows обрезает длинный текст, и список туда всё равно не влезет.
    if let Some(group) = groups.first() {
        if let Some(now) = &group.now {
            text.push_str(&format!("\n{}: {now}", group.name));
        }
    }
    text
}

/// Отпечаток содержимого меню: если он не изменился, пересобирать нечего.
fn signature(run: RunSummary, connected: bool, groups: &[TrayGroup]) -> String {
    let mut parts = vec![format!(
        "{}{}{}{}/{connected}",
        run.running as u8, run.known as u8, run.can_start as u8, run.pending as u8
    )];
    for group in groups {
        parts.push(format!(
            "{}={}|{}",
            group.name,
            group.now.as_deref().unwrap_or(""),
            group.nodes.join(",")
        ));
    }
    parts.join(";")
}

/// Базовая иконка приложения, декодированная один раз.
fn base_icon() -> Option<&'static (Vec<u8>, u32, u32)> {
    static CACHE: OnceLock<Option<(Vec<u8>, u32, u32)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let image = Image::from_bytes(include_bytes!("../icons/32x32.png")).ok()?;
            Some((image.rgba().to_vec(), image.width(), image.height()))
        })
        .as_ref()
}

/// Активное состояние — обычная иконка, неактивное — она же в сером и
/// полупрозрачном виде. Пользователю не приходится наводить курсор, чтобы
/// понять, работает ли туннель.
fn icon_for(active: bool) -> Image<'static> {
    let Some((rgba, width, height)) = base_icon() else {
        // До иконки не добрались — рисуем однотонный квадрат, чтобы в трее
        // хоть что-то было видно.
        let fill = if active { [0x6f, 0x9c, 0xff, 0xff] } else { [0x7b, 0x84, 0x94, 0xb0] };
        return Image::new_owned(fill.repeat(16 * 16), 16, 16);
    };

    if active {
        return Image::new_owned(rgba.clone(), *width, *height);
    }

    let mut dimmed = rgba.clone();
    for pixel in dimmed.chunks_exact_mut(4) {
        let luma = (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32)
            .round()
            .clamp(0.0, 255.0) as u8;
        pixel[0] = luma;
        pixel[1] = luma;
        pixel[2] = luma;
        pixel[3] = (pixel[3] as u16 * 110 / 255) as u8;
    }
    Image::new_owned(dimmed, *width, *height)
}
