//! The wrapper service: SCM starts this same binary with the `--scm` flag, and
//! the wrapper brings up sing-box as a child process and reports the state to SCM.
//!
//! `sing-box run` is a console program: it does not implement the Service
//! Control Manager protocol and does not call `SetServiceStatus`. Registering
//! it as a service directly does not work — SCM waits 60 seconds and returns
//! error 1053 ("service did not respond … in a timely fashion"). So we register
//! ourselves as the service: we talk to SCM, start sing-box as a child, and
//! proxy STOP/SHUTDOWN to it by killing the process.
//!
//! This is exactly what NSSM/WinSW do, but without an external dependency: the
//! wrapper lives in the main binary and is selected by a single command-line flag.
//!
//! ## Control flow
//!
//! `register` cannot be called directly from `main`:
//! `RegisterServiceCtrlHandlerExW` requires the thread to have been started by
//! the dispatcher via `StartServiceCtrlDispatcherW`, otherwise it fails with
//! 1063 (`ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`). So `main` calls
//! [`dispatch`], which connects the process to SCM (and blocks until the
//! service stops), and SCM then calls [`service_main`] on a separate thread —
//! and only from there do we register the handler and report the status.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{register, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use super::SERVICE_NAME;
use crate::error::{Error, Result};

/// The command-line flag that enables the wrapper-service mode.
pub const FLAG: &str = "--scm";

/// Whether the binary was started by SCM as a service (then Tauri should not be brought up).
pub fn is_invocation() -> bool {
    std::env::args().any(|a| a == FLAG)
}

/// The wrapper arguments after `--scm`: the sing-box path, the runtime config, the data dir.
pub struct ScmArgs {
    pub sing_box: PathBuf,
    pub config: PathBuf,
    pub data_dir: PathBuf,
}

/// Parse `--scm <sing-box> <config> <data-dir>` from the command line.
fn parse_args() -> Result<ScmArgs> {
    let mut it = std::env::args().skip_while(|a| a != FLAG).skip(1);
    let mut nth = |label: &str| {
        it.next().ok_or_else(|| Error::Other(format!("--scm: argument \"{label}\" was not provided")))
    };
    Ok(ScmArgs {
        sing_box: PathBuf::from(nth("sing-box path")?),
        config: PathBuf::from(nth("config path")?),
        data_dir: PathBuf::from(nth("data-dir")?),
    })
}

/// Whether SCM asked us to stop. Set by the handler below.
static STOP: AtomicBool = AtomicBool::new(false);

/// The entry point SCM calls on a separate thread after [`dispatch`] connected
/// the process to the dispatcher. Here we register the control-signal handler
/// and bring up sing-box. Errors go to `scm.log` — stderr goes nowhere under
/// SCM, otherwise the cause is lost.
fn service_main(_arguments: Vec<OsString>) {
    let _ = scm_log("service_main: entry");
    if let Err(e) = run() {
        let _ = scm_log(&format!("service_main: error — {e}"));
        eprintln!("vantage-box scm: {e}");
    }
    let _ = scm_log("service_main: exit");
}

define_windows_service!(ffi_service_main, service_main);

/// Connects the process to SCM and blocks the current thread until the service
/// stops. Called from `main` (`lib::run`) on `--scm`. On a separate thread SCM
/// runs [`service_main`].
pub fn dispatch() {
    let _ = scm_log("dispatch: connecting to SCM");
    if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        // 1063 and the like — the process was not started by SCM as a service.
        // Log it: this is the only way to see the cause if the wrapper was
        // started manually or SCM refused to connect.
        let _ = scm_log(&format!("dispatch: could not connect to SCM — {}", winerr(&e)));
        eprintln!("vantage-box scm dispatch: {e}");
    }
    let _ = scm_log("dispatch: returning from the dispatcher");
}

/// Registers with SCM, brings up sing-box as a child, and keeps the service
/// alive while sing-box is running or until SCM asks to stop. Returns an error
/// if sing-box exited with a non-zero code — SCM records it in the service state.
fn run() -> Result<()> {
    STOP.store(false, Ordering::SeqCst);
    let args = parse_args()?;
    let _ = scm_log(&format!(
        "run: sing-box={} config={} data={}",
        args.sing_box.display(),
        args.config.display(),
        args.data_dir.display()
    ));

    // The SCM control-signal handler. Nothing needs to be captured: the stop
    // flag lives in a static, so the closure implements Fn + Send.
    let handler = |control: ServiceControl| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                STOP.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status = register(SERVICE_NAME, handler)
        .map_err(|e| Error::Other(format!("SCM: could not register the handler — {}", winerr(&e))))?;

    let report = |state: ServiceState, checkpoint: u32, wait_hint: Duration| -> Result<()> {
        status
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: if matches!(state, ServiceState::Running) {
                    ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
                } else {
                    ServiceControlAccept::empty()
                },
                exit_code: ServiceExitCode::NO_ERROR,
                checkpoint,
                wait_hint,
                process_id: None,
            })
            .map_err(|e| Error::Other(format!("SCM: could not update the status — {}", winerr(&e))))
    };

    report(ServiceState::StartPending, 1, Duration::from_secs(20))?;

    // sing-box output goes to the same log as in the process mode: if sing-box
    // crashes at startup, the cause is here, not in the API (which never comes up).
    std::fs::create_dir_all(&args.data_dir).ok();
    let log = args.data_dir.join("sing-box.log");
    let out = std::fs::File::create(&log).map_err(|e| Error::io(log.display().to_string(), e))?;
    let err = out
        .try_clone()
        .map_err(|e| Error::io(log.display().to_string(), e))?;

    let mut command = Command::new(&args.sing_box);
    command
        .arg("run")
        .arg("-c")
        .arg(&args.config)
        .arg("-D")
        .arg(&args.data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    hide_console(&mut command);

    let mut child = command
        .spawn()
        .map_err(|e| Error::io(args.sing_box.display().to_string(), e))?;

    report(ServiceState::Running, 0, Duration::ZERO)?;
    let _ = scm_log("run: sing-box started, service Running");

    // Keep the service alive while sing-box is running. We leave the loop when:
    //  * SCM sent STOP — then we kill the child ourselves;
    //  * sing-box exited on its own — for example, crashed at startup on a bad config.
    let exit_code = loop {
        if STOP.load(Ordering::SeqCst) {
            report(ServiceState::StopPending, 2, Duration::from_secs(20))?;
            let _ = child.kill();
            let _ = child.wait();
            break 0;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(1),
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => break 1,
        }
    };

    let _ = report(ServiceState::Stopped, 0, Duration::ZERO);
    let _ = scm_log(&format!("run: sing-box exited with code {exit_code}"));

    if exit_code == 0 {
        Ok(())
    } else {
        Err(Error::Other(format!("sing-box exited with code {exit_code}")))
    }
}

/// Extracts the OS code from a `windows_service` error — by default its
/// `Display` prints a generic "IO error in winapi call" and swallows the inner
/// `io::Error`, which makes diagnosis impossible.
fn winerr(e: &windows_service::Error) -> String {
    match e {
        windows_service::Error::Winapi(io) => {
            format!("winapi error (code {}): {io}", io.raw_os_error().unwrap_or(-1))
        }
        other => other.to_string(),
    }
}

/// SCM starts the process without a console, so `eprintln!` goes nowhere. This
/// log is the only way to see where the wrapper tripped (especially before
/// `sing-box.log` is created). It lives next to the settings:
/// `%APPDATA%\vantage-box\scm.log`.
fn scm_log(message: &str) -> std::io::Result<()> {
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("vantage-box"))
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir).ok();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("scm.log"))?;
    writeln!(f, "{message}")?;
    Ok(())
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}