//! Tray icon: state at a glance and control without opening the window.
//!
//! The menu is rebuilt only when its content changes — otherwise the
//! background refresh every few seconds would make the menu flicker.

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

/// How often we pull state into the tray.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// How many nodes we show in a group's submenu. A tray menu with a hundred
/// items is unreadable, and long lists are a job for the window.
const MAX_NODES_PER_GROUP: usize = 24;

// Menu item identifiers.
const ID_TOGGLE: &str = "toggle";
const ID_RESTART: &str = "restart";
const ID_SHOW: &str = "show";
const ID_QUIT: &str = "quit";
/// Proxy selection items are numbered: sing-box tags can contain anything,
/// including separators, so we do not encode the group name in the id.
const ID_SELECT_PREFIX: &str = "select:";

/// What the tray is currently showing. Needed to avoid rebuilding the menu in vain.
#[derive(Default)]
pub struct TrayRegistry {
    /// item id → (group, node).
    selections: Mutex<HashMap<String, (String, String)>>,
    /// Fingerprint of the last built menu.
    signature: Mutex<Option<String>>,
}

/// Creates the tray icon. The menu is populated by the first `refresh`.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    // A second call to manage state panics, so setup is meant to be called
    // once — and it is only called once at startup.
    app.manage(TrayRegistry::default());

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_for(false))
        .tooltip("Vantage Box")
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            // Left click opens the window, right click shows the menu (drawn by the OS).
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

/// Background tray refresh. Lives as long as the application.
pub fn spawn_refresher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app).await;
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });
}

/// Rebuilds the icon, tooltip, and menu to match the current state.
pub async fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let status = actions::run_status(app).await.ok();
    let run = RunSummary::from(status.as_ref());

    let connection = app.state::<AppState>().streams.status();
    let connected = connection.state == ConnectionState::Connected;

    // Only query groups when the API is alive: otherwise it is a guaranteed
    // timeout every few seconds.
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
        eprintln!("failed to update tray menu: {e}");
    }
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

/// A selector group as the tray shows it.
struct TrayGroup {
    name: String,
    now: Option<String>,
    nodes: Vec<String>,
    /// How many nodes did not fit.
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

    // The source is a HashMap, so without sorting the item order would change
    // from one refresh to the next.
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    groups
}

/// Everything the tray needs to know about sing-box state. A copy, not a
/// reference to `RunStatus`: it must stay `Copy` — it is passed around every closure.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RunSummary {
    running: bool,
    /// Whether the state could be read at all.
    known: bool,
    /// Start is possible: either the service is installed, or the config does not need TUN.
    can_start: bool,
    /// A transition between states is in progress — nothing to control right now.
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

    // Group submenus. Keep them alive until build() is called: SubmenuBuilder
    // returns values we reference further down.
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
                format!("…{} more — open the window", group.hidden),
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

    let toggle_text = if run.running { "Stop" } else { "Start" };
    // While a transition is in progress, the command would not run anyway. Starting without
    // a service is only impossible for a config with TUN — stopping is always available.
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
            "Soft restart",
            run.running && !run.pending,
            None::<&str>,
        )?)
        .separator()
        .item(&MenuItem::with_id(
            app,
            ID_SHOW,
            "Open Vantage Box",
            true,
            None::<&str>,
        )?)
        .separator()
        .item(&MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?)
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
                    // The menu could have gone stale between rendering and the click.
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        };

        if let Err(e) = result {
            eprintln!("tray action failed: {e}");
        }
        refresh(&app).await;
    });
}

// ---------------------------------------------------------------------------
// Labels and icons
// ---------------------------------------------------------------------------

fn state_label(run: RunSummary, connected: bool) -> &'static str {
    if !run.known {
        return "state unknown";
    }
    match run {
        RunSummary { running: true, .. } if connected => "running",
        RunSummary { running: true, .. } => "started, no API connection",
        RunSummary { pending: true, .. } => "switching",
        RunSummary {
            can_start: false, ..
        } => "config needs TUN — install the service",
        _ => "stopped",
    }
}

fn tooltip(run: RunSummary, connected: bool, groups: &[TrayGroup]) -> String {
    let mut text = format!("Vantage Box — {}", state_label(run, connected));

    // Only the active outbound of the first group goes into the tooltip: the
    // Windows tooltip truncates long text, and the full list would not fit anyway.
    if let Some(group) = groups.first() {
        if let Some(now) = &group.now {
            text.push_str(&format!("\n{}: {now}", group.name));
        }
    }
    text
}

/// Fingerprint of the menu contents: if unchanged, nothing to rebuild.
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

/// The tray icon in one of two states, decoded once.
/// `active = false` — default ("off"), `active = true` — "on".
/// Icons are generated from a Figma export by the `npm run icons` script
/// (scripts/generate-icons.mjs): tray-off.png / tray-on.png in src-tauri/icons.
fn tray_icon(active: bool) -> Option<&'static (Vec<u8>, u32, u32)> {
    static OFF: OnceLock<Option<(Vec<u8>, u32, u32)>> = OnceLock::new();
    static ON: OnceLock<Option<(Vec<u8>, u32, u32)>> = OnceLock::new();
    let cache = if active { &ON } else { &OFF };
    let bytes: &'static [u8] = if active {
        include_bytes!("../icons/tray-on.png")
    } else {
        include_bytes!("../icons/tray-off.png")
    };
    cache
        .get_or_init(|| {
            let image = Image::from_bytes(bytes).ok()?;
            Some((image.rgba().to_vec(), image.width(), image.height()))
        })
        .as_ref()
}

/// Active state → the "on" icon, inactive → the default "off" icon.
/// The user does not have to hover to tell whether the tunnel is working.
fn icon_for(active: bool) -> Image<'static> {
    if let Some((rgba, width, height)) = tray_icon(active) {
        return Image::new_owned(rgba.clone(), *width, *height);
    }
    // Icon not available — draw a solid square so at least something is
    // visible in the tray. Colors match the accents of the original icons.
    let fill = if active { [0x4d, 0xbf, 0x45, 0xff] } else { [0x7b, 0x84, 0x94, 0xb0] };
    Image::new_owned(fill.repeat(16 * 16), 16, 16)
}
