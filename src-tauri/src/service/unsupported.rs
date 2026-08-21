//! A stub for platforms that ServiceController has not reached yet (M3:
//! systemd on Linux, launchd on macOS). The app stays functional — it can
//! manage an already-running sing-box through the Clash API.

use super::{ServiceInfo, ServiceState, SERVICE_NAME};
use crate::error::{Error, Result};
use crate::settings::Settings;

fn not_supported() -> Error {
    Error::Other(
        "service management is not yet implemented on this OS — start sing-box yourself".into(),
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
