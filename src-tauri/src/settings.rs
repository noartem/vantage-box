//! `settings.json` — единственный источник правды для конфигурации GUI.
//!
//! Файл лежит в стандартной директории настроек ОС, читается как JSONC,
//! пишется атомарно и отслеживается через `notify`: ручные правки в редакторе
//! подхватываются на лету.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;

/// Имя директории приложения внутри системной конфиг-папки.
const APP_DIR: &str = "vantage-box";
const FILE_NAME: &str = "settings.json";

/// Окно, за которое схлопываются подряд идущие события файловой системы.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Модель настроек
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Ссылка на JSON Schema — чтобы редакторы давали автокомплит.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub sing_box: SingBoxSettings,
    pub clash_api: ClashApiSettings,
    pub ui: UiSettings,
    pub tray: TraySettings,
    pub hotkeys: HotkeySettings,
    /// Запускать Vantage Box при входе в систему.
    pub autostart: bool,
    /// Автообновление самого GUI (не sing-box). Отдельно от `sing_box.update_policy`.
    pub gui_update: GuiUpdateSettings,
    /// Подписки на списки прокси.
    pub subscriptions: Vec<SubscriptionSettings>,
    /// Автопереключение selector-групп на резерв при падении активного узла.
    pub fallback: FallbackSettings,
}

/// Политика автообновления GUI-приложения. Переиспользует те же значения,
/// что и `sing_box.update_policy`, но живёт отдельно: бинарник и оболочку
/// обновляют независимо.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GuiUpdateSettings {
    pub policy: UpdatePolicy,
}

/// Одна подписка: URL, отдающий список прокси, и куда их вливать.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubscriptionSettings {
    /// Уникальный идентификатор в пределах списка. Не отдаётся провайдером —
    /// генерируем сами, чтобы UI и бэкенд могли ссылаться на подписку стабильно.
    pub id: String,
    /// Человекочитаемое имя.
    pub name: String,
    /// URL подписки.
    pub url: String,
    /// Включена ли подписка (выключенная не обновляется и не вливается).
    pub enabled: bool,
    /// Тег selector-группы, в чей `outbounds` дописать узлы. `None` — во все
    /// selector/urltest-группы.
    pub target_group: Option<String>,
    /// Как часто перетягивать подписку, часы.
    pub update_interval: u64,
}

/// Автопереключение selector-групп: если активный узел падает, выбираем резерв.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FallbackSettings {
    pub enabled: bool,
    /// Как часто пингуем активный узел, секунды.
    pub interval_sec: u32,
    /// Таймаут пинга, мс.
    pub timeout_ms: u32,
    /// Задержка выше этой считается «плохой» — переключаем. 0 — только по
    /// полной недоступности.
    pub max_delay_ms: u32,
    /// Теги групп, за которыми следим. Пусто — за всеми selector-группами.
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SingBoxSettings {
    /// Путь к пользовательскому `config.json` sing-box.
    pub config_path: String,
    /// Путь к бинарнику. Пусто — используем бинарник под управлением Vantage Box.
    pub binary_path: String,
    /// Политика автообновления бинарника.
    pub update_policy: UpdatePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePolicy {
    Off,
    Notify,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClashApiSettings {
    /// Базовый адрес Clash API. Строго loopback.
    pub url: String,
    /// Secret из конфига sing-box. Пусто — если API открыт без авторизации.
    pub secret: String,
    /// Уровень, на котором подписываемся на `/logs`.
    pub log_level: LogLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiSettings {
    pub theme: Theme,
    /// URL, по которому измеряется задержка outbound'ов.
    pub latency_test_url: String,
    /// Таймаут latency-теста, мс.
    pub latency_test_timeout: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TraySettings {
    pub enabled: bool,
    /// Закрытие окна сворачивает в трей вместо выхода.
    pub close_to_tray: bool,
    pub start_minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HotkeySettings {
    /// Попап выбора прокси.
    pub proxy_popup: String,
    /// Включить/выключить sing-box.
    pub toggle: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: Some("./settings.schema.json".into()),
            sing_box: SingBoxSettings::default(),
            clash_api: ClashApiSettings::default(),
            ui: UiSettings::default(),
            tray: TraySettings::default(),
            hotkeys: HotkeySettings::default(),
            autostart: false,
            gui_update: GuiUpdateSettings::default(),
            subscriptions: Vec::new(),
            fallback: FallbackSettings::default(),
        }
    }
}

impl Default for GuiUpdateSettings {
    fn default() -> Self {
        Self {
            policy: UpdatePolicy::Notify,
        }
    }
}

impl Default for SubscriptionSettings {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            enabled: true,
            target_group: None,
            update_interval: 24,
        }
    }
}

impl Default for FallbackSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_sec: 60,
            timeout_ms: 5000,
            max_delay_ms: 0,
            groups: Vec::new(),
        }
    }
}

impl Default for SingBoxSettings {
    fn default() -> Self {
        Self {
            config_path: String::new(),
            binary_path: String::new(),
            update_policy: UpdatePolicy::Notify,
        }
    }
}

/// Адрес Clash API по умолчанию.
///
/// Порт 9090 — общий стандарт для всех клиентов на базе Clash, и он же
/// стандартный порт Prometheus. На машине, где уже крутится что-то из этого,
/// два процесса дерутся за один порт. Свой порт снимает вопрос целиком.
pub const DEFAULT_CLASH_URL: &str = "http://127.0.0.1:9797";

/// Порт, который занимают все прочие клиенты Clash.
const SHARED_CLASH_PORT: &str = ":9090";

/// Приводит настройки к рабочему виду после чтения с диска.
///
/// Единственное правило: стандартный клешовый порт уводим на свой. Это не
/// вкусовщина — на 9090 приложение имеет все шансы подключиться не к тому
/// sing-box, который запустило само.
fn normalize(mut settings: Settings) -> Settings {
    if settings
        .clash_api
        .url
        .trim()
        .trim_end_matches('/')
        .ends_with(SHARED_CLASH_PORT)
    {
        settings.clash_api.url = DEFAULT_CLASH_URL.into();
    }
    settings
}

impl Default for ClashApiSettings {
    fn default() -> Self {
        Self {
            url: DEFAULT_CLASH_URL.into(),
            secret: String::new(),
            log_level: LogLevel::Info,
        }
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            latency_test_url: "http://www.gstatic.com/generate_204".into(),
            latency_test_timeout: 5000,
        }
    }
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            close_to_tray: true,
            start_minimized: false,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            proxy_popup: "Ctrl+Alt+P".into(),
            toggle: "Ctrl+Alt+O".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Пути
// ---------------------------------------------------------------------------

/// Переменная окружения, уводящая всё состояние приложения в другую папку.
/// Нужна интеграционным тестам, чтобы они гарантированно не трогали рабочую
/// конфигурацию пользователя.
pub const CONFIG_DIR_ENV: &str = "VANTAGE_BOX_CONFIG_DIR";

/// Директория настроек: `%APPDATA%/vantage-box`, `~/.config/vantage-box`,
/// `~/Library/Application Support/vantage-box`.
pub fn config_dir() -> Result<PathBuf> {
    if let Some(custom) = std::env::var_os(CONFIG_DIR_ENV) {
        if !custom.is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }

    dirs::config_dir()
        .map(|d| d.join(APP_DIR))
        .ok_or(Error::NoConfigDir)
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(FILE_NAME))
}

// ---------------------------------------------------------------------------
// Чтение и запись
// ---------------------------------------------------------------------------

/// Читает настройки с диска. Отсутствующий файл — не ошибка: берём дефолты
/// и создаём файл, чтобы пользователю было что править.
pub fn load_or_create() -> Result<Settings> {
    let path = settings_path()?;
    if !path.exists() {
        let defaults = Settings::default();
        write_to_disk(&path, &defaults)?;
        write_schema()?;
        return Ok(defaults);
    }

    let settings = read_from_disk(&path)?;
    // Если чтение что-то поправило (например, увело порт с общего 9090),
    // файл должен сразу это отражать: пользователь смотрит именно в него.
    if let Ok(raw) = read_raw(&path) {
        if raw != settings {
            write_to_disk(&path, &settings)?;
        }
    }
    Ok(settings)
}

fn read_raw(path: &Path) -> Result<Settings> {
    let display = path.display().to_string();
    let raw = std::fs::read_to_string(path).map_err(|e| Error::io(&display, e))?;
    if raw.trim().is_empty() {
        return Ok(Settings::default());
    }
    serde_json::from_str(&strip_jsonc(&raw)).map_err(|e| Error::parse(&display, e))
}

pub fn read_from_disk(path: &Path) -> Result<Settings> {
    // Пустой файл трактуем как «всё по умолчанию» — иначе первый же
    // `truncate` из редактора уронил бы загрузку.
    read_raw(path).map(normalize)
}

/// Атомарная запись: пишем во временный файл рядом и переименовываем поверх.
/// Так редактор/watcher никогда не увидят полупустой файл.
pub fn write_to_disk(path: &Path, settings: &Settings) -> Result<()> {
    let display = path.display().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let mut body = serde_json::to_string_pretty(settings)
        .map_err(|e| Error::parse(&display, e))?;
    body.push('\n');

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(&display, e))?;
    Ok(())
}

/// Кладёт JSON Schema рядом с настройками — на неё ссылается `$schema`,
/// поэтому автокомплит работает в любом редакторе офлайн.
pub fn write_schema() -> Result<()> {
    let path = config_dir()?.join("settings.schema.json");
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    std::fs::write(&path, include_str!("../schemas/settings.schema.json"))
        .map_err(|e| Error::io(path.display().to_string(), e))
}

// ---------------------------------------------------------------------------
// Хранилище в памяти + watcher
// ---------------------------------------------------------------------------

/// Актуальные настройки, разделяемые между командами и фоновыми задачами.
pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    pub fn new(path: PathBuf, initial: Settings) -> Self {
        Self {
            path,
            current: RwLock::new(initial),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.read().expect("settings lock").clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Пишет на диск и обновляет память. Watcher увидит собственную запись,
    /// но сравнение со значением в памяти его погасит.
    pub fn save(&self, next: Settings) -> Result<()> {
        write_to_disk(&self.path, &next)?;
        *self.current.write().expect("settings lock") = next;
        Ok(())
    }

    /// Перечитывает файл. `Ok(None)` — содержимое не изменилось.
    pub fn reload(&self) -> Result<Option<Settings>> {
        let next = read_from_disk(&self.path)?;
        let mut guard = self.current.write().expect("settings lock");
        if *guard == next {
            return Ok(None);
        }
        *guard = next.clone();
        Ok(Some(next))
    }
}

/// Событие от watcher'а: файл изменился и его содержимое действительно другое.
pub type ChangeTx = tokio::sync::mpsc::UnboundedSender<()>;

/// Следит за файлом настроек. Watcher вешаем на *директорию*: атомарная запись
/// заменяет inode, и подписка на сам файл после первой же записи умирает.
///
/// Возвращённый watcher нужно держать живым — при drop'е слежка прекращается.
pub fn spawn_watcher(path: &Path, tx: ChangeTx) -> Result<notify::RecommendedWatcher> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Other("у файла настроек нет родительской директории".into()))?
        .to_path_buf();
    let target = path.to_path_buf();

    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<()>();

    let mut watcher = notify::recommended_watcher(
        move |res: std::result::Result<Event, notify::Error>| {
            let Ok(event) = res else { return };
            // Временный файл атомарной записи нас не интересует.
            let touches_target = event.paths.iter().any(|p| p == &target);
            if touches_target {
                let _ = raw_tx.send(());
            }
        },
    )
    .map_err(|e| Error::Other(format!("не удалось создать watcher: {e}")))?;

    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| Error::Other(format!("не удалось следить за {}: {e}", dir.display())))?;

    // Дебаунс в отдельном потоке: сохранение из редактора — это обычно
    // несколько событий подряд (truncate, write, rename).
    std::thread::spawn(move || {
        while raw_rx.recv().is_ok() {
            // Схлопываем всё, что прилетело в окно дебаунса.
            while raw_rx.recv_timeout(WATCH_DEBOUNCE).is_ok() {}
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    Ok(watcher)
}

pub type SharedSettings = Arc<SettingsStore>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_off_shared_clash_port() {
        let mut settings = Settings::default();
        settings.clash_api.url = "http://127.0.0.1:9090".into();
        assert_eq!(normalize(settings).clash_api.url, DEFAULT_CLASH_URL);

        let mut settings = Settings::default();
        settings.clash_api.url = "http://127.0.0.1:9090/".into();
        assert_eq!(normalize(settings).clash_api.url, DEFAULT_CLASH_URL);
    }

    #[test]
    fn keeps_deliberate_port() {
        let mut settings = Settings::default();
        settings.clash_api.url = "http://127.0.0.1:18080".into();
        assert_eq!(
            normalize(settings).clash_api.url,
            "http://127.0.0.1:18080"
        );
    }
}
