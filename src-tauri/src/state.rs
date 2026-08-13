//! Глобальное состояние приложения.
//!
//! Здесь ровно две вещи: настройки и подключение к Clash API. Состояние
//! рантайма sing-box мы не кэшируем — источник правды всегда сам sing-box.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::clash::{ClashClient, StreamManager};
use crate::error::Result;
use crate::settings::{ChangeTx, SettingsStore, Settings, SharedSettings};

fn digest(content: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.finalize().into()
}

/// Событие для UI: `settings.json` изменился (нами или руками в редакторе).
pub const EVENT_SETTINGS: &str = "settings://changed";
/// Событие для UI: `settings.json` не читается (сломанный JSON и т.п.).
pub const EVENT_SETTINGS_ERROR: &str = "settings://error";
/// Событие для UI: пользовательский `config.json` изменился снаружи.
pub const EVENT_CONFIG_CHANGED: &str = "singbox://config-changed";

pub struct AppState {
    pub settings: SharedSettings,
    /// Пересобирается при изменении секции `clashApi`.
    pub client: RwLock<ClashClient>,
    pub streams: Arc<StreamManager>,
    /// Watcher должен жить столько же, сколько приложение: при drop'е слежка
    /// за файлом настроек молча прекращается.
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// Отдельная слежка за `config.json` sing-box: путь к нему меняется вместе
    /// с настройками, поэтому watcher приходится перевешивать.
    pub config_watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub config_watch_tx: ChangeTx,
    /// Хеш последнего содержимого, записанного нами. Нужен, чтобы собственная
    /// запись не выглядела как правка файла снаружи.
    pub config_signature: Mutex<Option<[u8; 32]>>,
    /// Хоткеи, которые не удалось зарегистрировать, с причинами.
    pub hotkey_problems: Mutex<Vec<String>>,
}

impl AppState {
    pub fn new(
        settings: SharedSettings,
        client: ClashClient,
        streams: Arc<StreamManager>,
        config_watch_tx: ChangeTx,
    ) -> Self {
        Self {
            settings,
            client: RwLock::new(client),
            streams,
            watcher: Mutex::new(None),
            config_watcher: Mutex::new(None),
            config_watch_tx,
            config_signature: Mutex::new(None),
            hotkey_problems: Mutex::new(Vec::new()),
        }
    }

    /// Перевешивает слежку на текущий `config.json`. Пустой путь снимает её.
    pub fn rearm_config_watcher(&self, path: &str) {
        let path = path.trim();
        let mut slot = self.config_watcher.lock().expect("config watcher lock");

        if path.is_empty() {
            *slot = None;
            return;
        }

        match crate::settings::spawn_watcher(Path::new(path), self.config_watch_tx.clone()) {
            Ok(watcher) => *slot = Some(watcher),
            Err(e) => {
                *slot = None;
                eprintln!("watcher config.json не запущен: {e}");
            }
        }
    }

    /// Запоминает, что файл сейчас содержит именно это.
    pub fn remember_config(&self, content: &str) {
        *self.config_signature.lock().expect("signature lock") = Some(digest(content));
    }

    /// `true`, если содержимое файла отличается от последнего записанного нами.
    pub fn config_changed_externally(&self, path: &str) -> bool {
        let Ok(content) = std::fs::read_to_string(path) else {
            // Файл исчез или стал нечитаемым — это точно повод сказать.
            return true;
        };
        let known = *self.config_signature.lock().expect("signature lock");
        known != Some(digest(&content))
    }

    /// Снимок клиента для использования в async-командах: держать guard
    /// через `.await` нельзя, поэтому клонируем (внутри `reqwest::Client`,
    /// клон дешёвый и переиспользует пул соединений).
    pub fn client(&self) -> ClashClient {
        self.client.read().expect("client lock").clone()
    }
}

/// Применяет новые настройки к рантайму: пересобирает клиента и, если
/// изменилось подключение или уровень логов, перезапускает подписки.
pub fn apply_settings(
    app: &AppHandle,
    state: &AppState,
    previous: &Settings,
    next: &Settings,
) -> Result<()> {
    if previous.sing_box.config_path != next.sing_box.config_path {
        state.rearm_config_watcher(&next.sing_box.config_path);
    }

    if previous.hotkeys != next.hotkeys {
        crate::hotkeys::apply(app, next);
    }

    if previous.autostart != next.autostart {
        sync_autostart(app, next.autostart);
    }

    let connection_changed = previous.clash_api != next.clash_api;

    if connection_changed {
        let client = ClashClient::new(&crate::runtime::effective_api_settings(next))?;
        *state.client.write().expect("client lock") = client.clone();
        state.streams.restart(client, next.clash_api.log_level);
    }

    let _ = app.emit(EVENT_SETTINGS, next.clone());
    Ok(())
}

/// Пересоздаёт клиента и подписки принудительно — для кнопки «переподключиться».
pub fn reconnect(state: &AppState) -> Result<()> {
    let settings = state.settings.get();
    let client = ClashClient::new(&crate::runtime::effective_api_settings(&settings))?;
    *state.client.write().expect("client lock") = client.clone();
    state.streams.restart(client, settings.clash_api.log_level);
    Ok(())
}

/// Приводит автозапуск в соответствие с настройками.
///
/// Реальное состояние спрашиваем у системы: пользователь мог убрать запись
/// из автозагрузки мимо приложения, и тогда наш флаг врал бы.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();
    if manager.is_enabled().unwrap_or(false) == enabled {
        return;
    }

    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    if let Err(e) = result {
        eprintln!("не удалось изменить автозапуск: {e}");
    }
}

pub fn share(store: SettingsStore) -> SharedSettings {
    Arc::new(store)
}
