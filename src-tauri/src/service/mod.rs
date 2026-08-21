//! Managing the sing-box process as a system service.
//!
//! Key idea: the GUI runs without administrator rights. Elevation is needed
//! exactly once — when registering the service — and at that point we also
//! grant the current user the right to start and stop exactly this service.
//! After that, start/stop go through without a single UAC prompt.

use serde::Serialize;

use crate::error::Result;
use crate::settings::Settings;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

/// The wrapper service for SCM: `sing-box run` is not service-aware on its
/// own, so we register ourselves as the service and proxy the lifecycle to the
/// child process.
#[cfg(windows)]
pub mod scm;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
use unsupported as platform;

/// The service name. ASCII and no spaces: it has to be passed to `sc.exe`.
pub const SERVICE_NAME: &str = "VantageBoxSingBox";
pub const SERVICE_DISPLAY_NAME: &str = "Vantage Box (sing-box)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// The service is not registered yet — one UAC prompt is needed.
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
    /// Whether there is an implementation for the current OS.
    pub supported: bool,
    pub state: ServiceState,
    /// Whether we have the rights to start and stop the service without elevation.
    pub can_control: bool,
    /// A note about the state — shown to the user as-is.
    pub detail: Option<String>,
}

impl ServiceInfo {
    pub fn is_running(&self) -> bool {
        self.state == ServiceState::Running
    }
}

/// The current state of the service. Does not require rights.
pub fn status() -> Result<ServiceInfo> {
    platform::status()
}

/// Registers (or recreates) the service. The only operation that needs UAC.
pub fn install(settings: &Settings) -> Result<()> {
    platform::install(settings)
}

/// Removes the service. Also requires elevation.
pub fn uninstall() -> Result<()> {
    platform::uninstall()
}

pub fn start() -> Result<()> {
    platform::start()
}

pub fn stop() -> Result<()> {
    platform::stop()
}
