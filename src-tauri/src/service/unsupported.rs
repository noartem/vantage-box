//! Заглушка для платформ, до которых ServiceController ещё не доехал (M3:
//! systemd на Linux, launchd на macOS). Приложение при этом остаётся рабочим —
//! оно умеет управлять уже запущенным sing-box через Clash API.

use super::{ServiceInfo, ServiceState, SERVICE_NAME};
use crate::error::{Error, Result};
use crate::settings::Settings;

fn not_supported() -> Error {
    Error::Other(
        "управление сервисом на этой ОС пока не реализовано — запустите sing-box самостоятельно"
            .into(),
    )
}

pub fn status() -> Result<ServiceInfo> {
    Ok(ServiceInfo {
        name: SERVICE_NAME.into(),
        supported: false,
        state: ServiceState::Unknown,
        can_control: false,
        detail: Some(not_supported().to_string()),
    })
}

pub fn install(_settings: &Settings) -> Result<()> {
    Err(not_supported())
}

pub fn uninstall() -> Result<()> {
    Err(not_supported())
}

pub fn start() -> Result<()> {
    Err(not_supported())
}

pub fn stop() -> Result<()> {
    Err(not_supported())
}
