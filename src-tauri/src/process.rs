//! Running sing-box as a child process — the mode without installing the service.
//!
//! The service is only needed where sing-box requires administrator rights:
//! TUN brings up a network adapter, and without elevation that does not work.
//! A configuration without TUN (a regular http/socks inbound) lives perfectly
//! fine as a regular process under the user — there is no reason to force a
//! service install for it.
//!
//! The process belongs to the application: it is stopped on quit, otherwise
//! after closing the GUI an unnamed sing-box would keep running with nothing
//! left to manage it.

use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::binary;
use crate::error::{Error, Result};
use crate::runtime;
use crate::settings::Settings;

/// The sing-box we started. `None` — the process was never started or has
/// already exited.
fn slot() -> &'static Mutex<Option<Child>> {
    static SLOT: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Where sing-box output goes. The same logs come through the Clash API, but if
/// the process crashes at startup the API does not come up in time — and the
/// only reason for the failure is visible only here.
pub fn log_path() -> Result<std::path::PathBuf> {
    Ok(binary::data_dir()?.join("sing-box.log"))
}

/// Whether our child process is running right now.
///
/// Also cleans up the slot: a finished process without a `wait` stays a zombie.
pub fn running() -> bool {
    let mut guard = slot().lock().expect("process lock");
    let Some(child) = guard.as_mut() else {
        return false;
    };
    match child.try_wait() {
        Ok(Some(_)) => {
            *guard = None;
            false
        }
        Ok(None) => true,
        // The process state is unknown — assume it is gone: otherwise the
        // "Start" button would stay disabled forever.
        Err(_) => {
            *guard = None;
            false
        }
    }
}

/// The PID of the running process — we show it in the UI as proof that
/// "running" refers to something concrete.
pub fn pid() -> Option<u32> {
    let mut guard = slot().lock().expect("process lock");
    let child = guard.as_mut()?;
    match child.try_wait() {
        Ok(None) => Some(child.id()),
        _ => {
            *guard = None;
            None
        }
    }
}

/// Starts sing-box with an already-prepared runtime config.
pub fn start(settings: &Settings) -> Result<()> {
    if running() {
        return Ok(());
    }

    let choice = binary::resolve(settings)?;
    if !choice.path.is_file() {
        return Err(Error::Other(format!(
            "sing-box file not found: {}",
            choice.path.display()
        )));
    }

    let prepared = runtime::prepare(settings)?;
    let data_dir = binary::data_dir()?;
    std::fs::create_dir_all(&data_dir).map_err(|e| Error::io(data_dir.display().to_string(), e))?;

    let log = log_path()?;
    let out = std::fs::File::create(&log).map_err(|e| Error::io(log.display().to_string(), e))?;
    let err = out
        .try_clone()
        .map_err(|e| Error::io(log.display().to_string(), e))?;

    let mut command = Command::new(&choice.path);
    command
        .arg("run")
        .arg("-c")
        .arg(&prepared.config_path)
        .arg("-D")
        .arg(&data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    hide_console(&mut command);

    let child = command
        .spawn()
        .map_err(|e| Error::io(choice.path.display().to_string(), e))?;

    *slot().lock().expect("process lock") = Some(child);

    // The process may drop right away — for example, the config was rejected.
    // Silently reporting "started" in that case would be a lie.
    std::thread::sleep(std::time::Duration::from_millis(600));
    if !running() {
        let detail = std::fs::read_to_string(&log).unwrap_or_default();
        let detail = tail(&detail, 12);
        return Err(Error::Other(if detail.is_empty() {
            "sing-box exited immediately after start".into()
        } else {
            format!("sing-box exited immediately after start:\n{detail}")
        }));
    }

    Ok(())
}

/// Stops the process. A missing process is not an error.
pub fn stop() -> Result<()> {
    let mut guard = slot().lock().expect("process lock");
    let Some(mut child) = guard.take() else {
        return Ok(());
    };

    if let Ok(Some(_)) = child.try_wait() {
        return Ok(());
    }

    child
        .kill()
        .map_err(|e| Error::Other(format!("failed to stop sing-box: {e}")))?;
    let _ = child.wait();
    Ok(())
}

/// The last `lines` lines — sing-box output at startup can be long, and the
/// reason for failure is always at the end.
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}
