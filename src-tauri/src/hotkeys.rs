//! Глобальные хоткеи. Биндинги живут в `settings.json` и перерегистрируются
//! на каждое изменение настроек.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::actions;
use crate::settings::Settings;
use crate::state::AppState;
use crate::window;

/// Событие для UI: список проблем с регистрацией хоткеев.
pub const EVENT_HOTKEYS: &str = "hotkeys://problems";

/// Что делает хоткей.
#[derive(Clone, Copy)]
enum Action {
    /// Запустить или остановить sing-box.
    Toggle,
    /// Показать попап выбора прокси.
    ProxyPopup,
}

/// Перерегистрирует все хоткеи под текущие настройки.
///
/// Возвращает список проблем: занятую другой программой комбинацию или опечатку
/// в настройках надо показать пользователю, а не молча проглотить — иначе
/// хоткей просто «не работает» без объяснений.
pub fn apply(app: &AppHandle, settings: &Settings) -> Vec<String> {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();

    let mut problems = Vec::new();
    let bindings = [
        (settings.hotkeys.toggle.trim(), Action::Toggle, "включить/выключить"),
        (
            settings.hotkeys.proxy_popup.trim(),
            Action::ProxyPopup,
            "попап выбора прокси",
        ),
    ];

    for (binding, action, description) in bindings {
        // Пустая строка — осознанный отказ от хоткея, это не ошибка.
        if binding.is_empty() {
            continue;
        }

        let result = shortcuts.on_shortcut(binding, move |app, _shortcut, event| {
            // Без этой проверки действие срабатывало бы дважды: на нажатии
            // и на отпускании.
            if event.state != ShortcutState::Pressed {
                return;
            }
            dispatch(app.clone(), action);
        });

        if let Err(e) = result {
            problems.push(format!("{description} ({binding}): {e}"));
        }
    }

    remember(app, &problems);
    problems
}

fn dispatch(app: AppHandle, action: Action) {
    match action {
        Action::ProxyPopup => window::toggle_popup(&app),
        Action::Toggle => {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = actions::toggle(&app).await {
                    eprintln!("хоткей включения не сработал: {e}");
                }
                crate::tray::refresh(&app).await;
            });
        }
    }
}

fn remember(app: &AppHandle, problems: &[String]) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.hotkey_problems.lock().expect("hotkey problems lock") = problems.to_vec();
    }
    let _ = app.emit(EVENT_HOTKEYS, problems.to_vec());
}
