//! Управление процессом sing-box как системным сервисом.
//!
//! Ключевая идея: GUI работает без прав администратора. Elevation нужен ровно
//! один раз — при регистрации сервиса, и там же мы выдаём текущему пользователю
//! право стартовать и останавливать именно этот сервис. Дальше start/stop идут
//! без единого UAC-запроса.

use serde::Serialize;

use crate::error::Result;
use crate::settings::Settings;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
use unsupported as platform;

/// Имя сервиса. ASCII и без пробелов: его приходится передавать в `sc.exe`.
pub const SERVICE_NAME: &str = "VantageBoxSingBox";
pub const SERVICE_DISPLAY_NAME: &str = "Vantage Box (sing-box)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// Сервис ещё не зарегистрирован — нужен один UAC-запрос.
    NotInstalled,
    Stopped,
    StartPending,
    Running,
    StopPending,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub name: String,
    /// Есть ли реализация под текущую ОС.
    pub supported: bool,
    pub state: ServiceState,
    /// Хватает ли прав стартовать и останавливать сервис без elevation.
    pub can_control: bool,
    /// Пояснение к состоянию — показываем пользователю как есть.
    pub detail: Option<String>,
}

impl ServiceInfo {
    pub fn is_running(&self) -> bool {
        self.state == ServiceState::Running
    }
}

/// Текущее состояние сервиса. Прав не требует.
pub fn status() -> Result<ServiceInfo> {
    platform::status()
}

/// Регистрирует (или пересоздаёт) сервис. Единственная операция с UAC.
pub fn install(settings: &Settings) -> Result<()> {
    platform::install(settings)
}

/// Удаляет сервис. Тоже требует elevation.
pub fn uninstall() -> Result<()> {
    platform::uninstall()
}

pub fn start() -> Result<()> {
    platform::start()
}

pub fn stop() -> Result<()> {
    platform::stop()
}
