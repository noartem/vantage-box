//! Operations on the service and proxies, shared between the UI and the tray.
//!
//! The tray and the window must behave the same, so the logic lives here, and
//! `commands.rs` and `tray.rs` are only thin wrappers over it.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::clash::models::ConnectionState;
use crate::clash::ClashClient;
use crate::error::{Error, Result};
use crate::process;
use crate::runtime;
use crate::service::{self, ServiceInfo, ServiceState};
use crate::settings::Settings;
use crate::state::{self, AppState};

/// Event for the UI: sing-box state changed (including from the tray).
pub const EVENT_SERVICE: &str = "service://changed";
/// Event for the UI: the selection in a selector group changed (including from the tray).
pub const EVENT_PROXIES: &str = "proxies://changed";

/// How long we wait for the Clash API after restarting sing-box.
const API_TIMEOUT: Duration = Duration::from_secs(20);

/// Blocking work (SCM, spawning processes) — not on async-runtime threads.
pub async fn blocking<T, F>(work: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| Error::Other(format!("background task aborted: {e}")))?
}

/// How exactly sing-box is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunMode {
    /// Via the system service — the only way to bring up TUN.
    Service,
    /// As a regular child process under the user.
    Process,
}

/// Full sing-box state for the UI and the tray.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub mode: RunMode,
    /// Whether sing-box is running — service or process, does not matter.
    pub running: bool,
    pub service: ServiceInfo,
    /// PID of the child process, if started outside the service.
    pub process_pid: Option<u32>,
    /// The config needs TUN, and therefore a service and admin rights.
    pub tun: bool,
    /// Why the config could not be read. When set, `tun` is treated as false.
    pub config_problem: Option<String>,
}

impl RunStatus {
    /// Whether it can be started right now: without a service, TUN cannot be brought up.
    pub fn can_start(&self) -> bool {
        self.mode == RunMode::Service || !self.tun
    }
}

pub async fn run_status(app: &AppHandle) -> Result<RunStatus> {
    let state = app.state::<AppState>();
    let settings = state.settings.get();
    // The Clash API answers only while sing-box is alive. SCM can lie (the
    // wrapper process may have died without stopping its child), and our own
    // process slot knows only what we started — the API is the one signal that
    // covers a sing-box started outside our control.
    let api_up = state.streams.status().state == ConnectionState::Connected;
    blocking(move || build_status(&settings, api_up)).await
}

/// The tunnel is up, but Vantage Box has no handle to the sing-box behind it:
/// the service says stopped, our process slot is empty, yet the API answers.
/// Such a process (an orphan from a crashed session, or a foreign one) cannot
/// be stopped from here — the UI must say so instead of pretending.
fn orphan_error() -> Error {
    Error::Other(
        "sing-box is running, but outside Vantage Box's control (an orphaned \
         process from an earlier session). Stop it in Task Manager and start \
         the service again."
            .into(),
    )
}

/// Same contract, but with an explicit `api_up` signal: `run_status` reads it
/// from the stream manager, the action paths probe it live before they decide.
fn build_status(settings: &Settings, api_up: bool) -> Result<RunStatus> {
    let service = service::status()?;
    let installed = service.state != ServiceState::NotInstalled;

    let (tun, config_problem) = match runtime::requires_tun(settings) {
        Ok(tun) => (tun, None),
        Err(e) => (false, Some(e.to_string())),
    };

    let process_pid = process::pid();

    Ok(RunStatus {
        mode: if installed {
            RunMode::Service
        } else {
            RunMode::Process
        },
        running: service.is_running() || process_pid.is_some() || api_up,
        service,
        process_pid,
        tun,
        config_problem,
    })
}

/// Start: rebuild the runtime config (a fresh secret on every start) and
/// reconnect to it immediately.
///
/// A registered service takes priority: since the user installed it, it is
/// the one that should manage sing-box. Without a service we start as a
/// process — but only if the config does not need TUN.
///
/// When the API answers but neither the service nor our slot owns sing-box,
/// starting a second instance would only collide on the API port — the state
/// is reported as-is instead.
pub async fn start(app: &AppHandle) -> Result<RunStatus> {
    let settings = app.state::<AppState>().settings.get();

    // Live probe, not the cached stream status: the start decision must not
    // depend on a snapshot that is up to 3 s stale.
    let api_up = app.state::<AppState>().client().version().await.is_ok();

    let was_running = blocking(move || {
        let status = build_status(&settings, api_up)?;

        if status.running {
            return Ok(true);
        }

        if status.mode == RunMode::Service {
            runtime::prepare(&settings)?;
            service::start()?;
        } else {
            if status.tun {
                return Err(Error::Other(
                    "the config needs TUN — that requires administrator rights. \
                     Install the service on the \"Service\" tab."
                        .into(),
                ));
            }

            process::start(&settings)?;
        }
        Ok(false)
    })
    .await?;

    // A fresh start gets a fresh secret: the client must be rebuilt before we
    // hit the API. When the tunnel was already up, reconnecting would only
    // reset the streams (a `Connecting` gap right before the announce) — the
    // client is already correct, so the state is announced as-is.
    if !was_running {
        state::reconnect(&app.state::<AppState>())?;
    }

    announce(app).await
}

/// Stop both: the service may have been installed after we started the
/// process — then "stop" must take down both.
///
/// If the API keeps answering after our own handles are closed, a sing-box we
/// do not own is still up. Saying "stopped" there would be a lie; the user is
/// told how to take the orphan down instead.
pub async fn stop(app: &AppHandle) -> Result<RunStatus> {
    blocking(|| {
        process::stop()?;
        let service = service::status()?;
        if service.state != ServiceState::NotInstalled {
            service::stop()?;
        }
        Ok(())
    })
    .await?;

    if app.state::<AppState>().client().version().await.is_ok() {
        return Err(orphan_error());
    }

    announce(app).await
}

/// Start if installed; stop if running. For the tray and the hotkey.
pub async fn toggle(app: &AppHandle) -> Result<RunStatus> {
    let current = run_status(app).await?;
    if current.running {
        stop(app).await
    } else {
        start(app).await
    }
}

pub async fn install(app: &AppHandle) -> Result<RunStatus> {
    let settings = app.state::<AppState>().settings.get();
    blocking(move || {
        // Reinstalling a running service breaks the VPN: the install script
        // first stops and deletes the old service. The first install (no
        // service yet) is not affected — it does not touch sing-box.
        let service = service::status()?;
        if service.is_running() {
            return Err(Error::Other(
                "sing-box is running as a service — stop it before reinstalling.".into(),
            ));
        }
        service::install(&settings)
    })
    .await?;
    announce(app).await
}

/// Removing a running service breaks the VPN, so while it is running we refuse:
/// let the user stop it first. Previously uninstall silently stopped the
/// service itself; now that is the user's responsibility.
pub async fn uninstall(app: &AppHandle) -> Result<RunStatus> {
    blocking(|| {
        let service = service::status()?;
        if service.is_running() {
            return Err(Error::Other(
                "sing-box is running as a service — stop it before removing.".into(),
            ));
        }
        service::uninstall()
    })
    .await?;
    announce(app).await
}

/// What could be preserved during a soft restart.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartOutcome {
    pub status: RunStatus,
    /// Groups whose selection was restored: `["group → node", …]`.
    pub restored: Vec<String>,
    /// Groups whose selection could not be restored, with a reason.
    pub skipped: Vec<String>,
    /// Whether the Clash API came up before the wait elapsed.
    pub api_back: bool,
}

/// Soft restart: take a snapshot of the current selector selections, restart
/// the service, and reapply the selections.
///
/// sing-box can already remember the selection via `experimental.cache_file` —
/// that is the first level. Restoring on top works regardless of whether the
/// cache is enabled, and fixes the case when it is off.
pub async fn restart(app: &AppHandle) -> Result<RestartOutcome> {
    let settings = app.state::<AppState>().settings.get();

    // Take the snapshot before stopping — after that there is no one to ask.
    let snapshot = snapshot_selection(&app.state::<AppState>().client()).await;

    // Same live probe as in start: a sing-box we do not own must fail fast,
    // not after 20 s of waiting for a service that will never come up.
    let api_up = app.state::<AppState>().client().version().await.is_ok();

    blocking(move || {
        let status = build_status(&settings, api_up)?;

        // The tunnel is up, but neither the service nor our slot owns it —
        // a "restart" has nothing to stop and would only spawn a second,
        // colliding instance.
        if status.running && !status.service.is_running() && status.process_pid.is_none() {
            return Err(orphan_error());
        }

        if status.mode == RunMode::Service {
            runtime::prepare(&settings)?;
            service::stop()?;
            return service::start();
        }

        if status.tun {
            return Err(Error::Other(
                "the config needs TUN — that requires administrator rights. \
                 Install the service on the \"Service\" tab."
                    .into(),
            ));
        }

        process::stop()?;
        process::start(&settings)
    })
    .await?;

    // The secret is different on the new run, so the client must be rebuilt
    // before we start hitting the API.
    state::reconnect(&app.state::<AppState>())?;

    let client = app.state::<AppState>().client();
    let api_back = wait_for_api(&client, API_TIMEOUT).await;

    let (restored, skipped) = if api_back {
        restore_selection(&client, snapshot).await
    } else {
        (
            Vec::new(),
            vec![format!(
                "Clash API did not come up within {} s — selection not restored",
                API_TIMEOUT.as_secs()
            )],
        )
    };

    Ok(RestartOutcome {
        status: announce(app).await?,
        restored,
        skipped,
        api_back,
    })
}

pub async fn select_proxy(app: &AppHandle, group: &str, name: &str) -> Result<()> {
    app.state::<AppState>().client().select(group, name).await?;
    let _ = app.emit(EVENT_PROXIES, ());
    Ok(())
}

/// Reads sing-box state and reports it to the window: the action may have
/// come from the tray, and the UI would otherwise only learn on the next poll.
async fn announce(app: &AppHandle) -> Result<RunStatus> {
    let info = run_status(app).await?;
    let _ = app.emit(EVENT_SERVICE, info.clone());
    Ok(info)
}

/// `group → selected node` for all groups that can be controlled by hand.
async fn snapshot_selection(client: &ClashClient) -> Vec<(String, String)> {
    let Ok(response) = client.proxies().await else {
        return Vec::new();
    };

    response
        .proxies
        .iter()
        .filter(|(name, proxy)| {
            proxy.is_group() && proxy.is_selectable() && name.as_str() != "GLOBAL"
        })
        .filter_map(|(name, proxy)| proxy.now.clone().map(|now| (name.clone(), now)))
        .collect()
}

async fn restore_selection(
    client: &ClashClient,
    snapshot: Vec<(String, String)>,
) -> (Vec<String>, Vec<String>) {
    let mut restored = Vec::new();
    let mut skipped = Vec::new();

    if snapshot.is_empty() {
        return (restored, skipped);
    }

    let current = match client.proxies().await {
        Ok(response) => response.proxies,
        Err(e) => {
            skipped.push(format!("failed to read /proxies: {e}"));
            return (restored, skipped);
        }
    };

    for (group, wanted) in snapshot {
        let Some(proxy) = current.get(&group) else {
            // The config may have changed between runs — that is normal.
            skipped.push(format!("{group}: group no longer exists"));
            continue;
        };

        if !proxy.all.as_ref().is_some_and(|all| all.contains(&wanted)) {
            skipped.push(format!("{group}: node \"{wanted}\" no longer exists"));
            continue;
        }

        if proxy.now.as_deref() == Some(wanted.as_str()) {
            // sing-box already restored the selection from cache_file — nothing to do.
            continue;
        }

        match client.select(&group, &wanted).await {
            Ok(()) => restored.push(format!("{group} → {wanted}")),
            Err(e) => skipped.push(format!("{group}: {e}")),
        }
    }

    (restored, skipped)
}

/// Wait for sing-box to bring up the Clash API after a restart.
async fn wait_for_api(client: &ClashClient, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if client.version().await.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}
