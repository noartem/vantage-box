//! Windows Service Control Manager.
//!
//! Installation goes through an elevated PowerShell script: `New-Service`
//! creates the service, and `sc.exe sdset` appends to its security descriptor
//! the right of the current user to start/stop it. After that, controlling the
//! service from the GUI does not require administrator rights.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use windows_service::service::{ServiceAccess, ServiceState as WinState};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use super::{ServiceInfo, ServiceState, SERVICE_DISPLAY_NAME, SERVICE_NAME};
use crate::binary;
use crate::error::{Error, Result};
use crate::runtime;
use crate::settings::{config_dir, Settings};

/// `ERROR_SERVICE_DOES_NOT_EXIST` — the one "normal" SCM failure.
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
/// `ERROR_CANCELLED` — the user closed the UAC prompt.
const ERROR_CANCELLED: i32 = 1223;

/// Extracts the OS code from a `windows_service` error. Its `Display` prints a
/// generic "IO error in winapi call" and swallows the inner `io::Error` —
/// without the code ("failed to start the service: IO error in winapi call")
/// the cause cannot be found.
fn winerr(e: &windows_service::Error) -> String {
    match e {
        windows_service::Error::Winapi(io) => {
            format!(
                "winapi error (code {}): {io}",
                io.raw_os_error().unwrap_or(-1)
            )
        }
        other => other.to_string(),
    }
}

/// How long we wait for the service to transition to the target state.
const TRANSITION_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

// ---------------------------------------------------------------------------
// Reading state
// ---------------------------------------------------------------------------

pub fn status() -> Result<ServiceInfo> {
    let Some(service) = open(ServiceAccess::QUERY_STATUS)? else {
        return Ok(ServiceInfo {
            name: SERVICE_NAME.into(),
            supported: true,
            state: ServiceState::NotInstalled,
            can_control: false,
            detail: None,
        });
    };

    let status = service
        .query_status()
        .map_err(|e| Error::Other(format!("failed to query service status: {}", winerr(&e))))?;

    // We check the rights with a separate open: if sdset did not run, a start
    // would hit an access-denied, and it is better to say so up front.
    let can_control = open(ServiceAccess::START | ServiceAccess::STOP)
        .map(|s| s.is_some())
        .unwrap_or(false);

    Ok(ServiceInfo {
        name: SERVICE_NAME.into(),
        supported: true,
        state: map_state(status.current_state),
        can_control,
        detail: (!can_control)
            .then(|| "no rights to manage the service — reinstall it to grant them".to_string()),
    })
}

fn map_state(state: WinState) -> ServiceState {
    match state {
        WinState::Stopped => ServiceState::Stopped,
        WinState::StartPending => ServiceState::StartPending,
        WinState::StopPending => ServiceState::StopPending,
        WinState::Running => ServiceState::Running,
        _ => ServiceState::Unknown,
    }
}

/// `Ok(None)` — the service is not registered. Everything else is a real error.
fn open(access: ServiceAccess) -> Result<Option<windows_service::service::Service>> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| Error::Other(format!("no access to the service manager: {}", winerr(&e))))?;

    match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => Ok(Some(service)),
        Err(windows_service::Error::Winapi(io))
            if io.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST) =>
        {
            Ok(None)
        }
        Err(e) => Err(Error::Other(format!(
            "failed to open the service: {}",
            winerr(&e)
        ))),
    }
}

// ---------------------------------------------------------------------------
// Start and stop (without administrator rights)
// ---------------------------------------------------------------------------

pub fn start() -> Result<()> {
    let service = open(ServiceAccess::START | ServiceAccess::QUERY_STATUS)?
        .ok_or_else(|| Error::Other("the service is not installed".into()))?;

    let current = service
        .query_status()
        .map_err(|e| Error::Other(format!("failed to query service status: {}", winerr(&e))))?;
    if current.current_state == WinState::Running {
        return Ok(());
    }

    service
        .start(&[] as &[&OsStr])
        .map_err(|e| Error::Other(format!("failed to start the service: {}", winerr(&e))))?;

    wait_for(&service, WinState::Running)
}

pub fn stop() -> Result<()> {
    let service = open(ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?
        .ok_or_else(|| Error::Other("the service is not installed".into()))?;

    let current = service
        .query_status()
        .map_err(|e| Error::Other(format!("failed to query service status: {}", winerr(&e))))?;
    if current.current_state == WinState::Stopped {
        return Ok(());
    }

    service
        .stop()
        .map_err(|e| Error::Other(format!("failed to stop the service: {}", winerr(&e))))?;

    wait_for(&service, WinState::Stopped)
}

/// SCM responds to start/stop immediately, but the state changes asynchronously.
fn wait_for(service: &windows_service::service::Service, target: WinState) -> Result<()> {
    let deadline = Instant::now() + TRANSITION_TIMEOUT;
    loop {
        let status = service
            .query_status()
            .map_err(|e| Error::Other(format!("failed to query service status: {}", winerr(&e))))?;

        if status.current_state == target {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(Error::Other(format!(
                "the service did not transition to {target:?} within {} s — see the sing-box logs",
                TRANSITION_TIMEOUT.as_secs()
            )));
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// Install and uninstall (one UAC prompt)
// ---------------------------------------------------------------------------

pub fn install(settings: &Settings) -> Result<()> {
    let choice = binary::resolve(settings)?;
    if !choice.path.is_file() {
        return Err(Error::Other(format!(
            "sing-box file not found: {}",
            choice.path.display()
        )));
    }

    // Prepare the runtime config in advance: the service will point straight at it.
    let prepared = runtime::prepare(settings)?;
    let data_dir = binary::data_dir()?;
    std::fs::create_dir_all(&data_dir).map_err(|e| Error::io(data_dir.display().to_string(), e))?;

    // We register not sing-box as the service (it does not speak to SCM — that
    // yields error 1053), but ourselves with the `--scm` flag: the wrapper
    // reports status to SCM and brings up sing-box as a child. Paths to
    // sing-box, the runtime config, and the data dir are passed as arguments.
    let wrapper = std::env::current_exe()
        .map_err(|e| Error::Other(format!("could not determine the executable path: {e}")))?;
    if !wrapper.is_file() {
        return Err(Error::Other(format!(
            "the Vantage Box executable was not found: {}",
            wrapper.display()
        )));
    }

    let sid = current_user_sid()?;
    let bin_path_name = format!(
        "\"{}\" {} \"{}\" \"{}\" \"{}\"",
        wrapper.display(),
        super::scm::FLAG,
        choice.path.display(),
        prepared.config_path,
        data_dir.display()
    );

    run_elevated("install", &install_script(&bin_path_name, &sid))
}

pub fn uninstall() -> Result<()> {
    run_elevated("uninstall", &uninstall_script())
}

fn install_script(bin_path_name: &str, sid: &str) -> String {
    // Recreate the service wholesale: Set-Service in Windows PowerShell 5.1
    // cannot change BinaryPathName, and we need to be able to reinstall with
    // new paths.
    format!(
        r#"$ErrorActionPreference = 'Stop'
$name = '{name}'
$binPath = '{bin}'
$sddl = '{sddl}'

if (Get-Service -Name $name -ErrorAction SilentlyContinue) {{
    Stop-Service -Name $name -Force -ErrorAction SilentlyContinue
    & sc.exe delete $name | Out-Null
    Start-Sleep -Milliseconds 700
}}

New-Service -Name $name `
    -BinaryPathName $binPath `
    -DisplayName '{display}' `
    -Description 'sing-box process managed by Vantage Box' `
    -StartupType Manual | Out-Null

& sc.exe sdset $name $sddl | Out-Null
if ($LASTEXITCODE -ne 0) {{ throw "sc sdset failed with code $LASTEXITCODE" }}
"#,
        name = SERVICE_NAME,
        bin = ps_quote(bin_path_name),
        sddl = ps_quote(&sddl_for(sid)),
        display = SERVICE_DISPLAY_NAME,
    )
}

fn uninstall_script() -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
$name = '{name}'
if (Get-Service -Name $name -ErrorAction SilentlyContinue) {{
    Stop-Service -Name $name -Force -ErrorAction SilentlyContinue
    & sc.exe delete $name | Out-Null
    if ($LASTEXITCODE -ne 0) {{ throw "sc delete failed with code $LASTEXITCODE" }}
}}
"#,
        name = SERVICE_NAME
    )
}

/// The security descriptor of the service: standard entries plus ours — it
/// grants a specific user the right to start (RP), stop (WP), and query
/// (LO, CC, LC) exactly this service.
fn sddl_for(sid: &str) -> String {
    format!(
        "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)\
         (A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)\
         (A;;CCLCSWLOCRRC;;;IU)\
         (A;;CCLCSWLOCRRC;;;SU)\
         (A;;CCLCSWRPWPDTLOCRRC;;;{sid})\
         S:(AU;FA;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;WD)"
    )
}

/// The SID of the current user. The API output is pure ASCII, locale does not interfere.
fn current_user_sid() -> Result<String> {
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-Command",
        "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
    ]);
    hide_console(&mut command);

    let output = command.output().map_err(|e| Error::io("powershell", e))?;

    let sid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !sid.starts_with("S-1-") {
        return Err(Error::Other(
            "could not determine the current user SID".into(),
        ));
    }
    Ok(sid)
}

/// Writes the script to disk and runs it elevated. One UAC prompt for the whole
/// script — no matter how many commands are inside it.
fn run_elevated(kind: &str, body: &str) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;

    let script = dir.join(format!("service-{kind}.ps1"));
    let log = dir.join(format!("service-{kind}.log"));
    let _ = std::fs::remove_file(&log);

    // The BOM is mandatory: without it Windows PowerShell 5.1 reads the file as
    // ANSI and breaks paths that contain Cyrillic in the user name.
    let mut content = String::from("\u{feff}");
    content.push_str(&wrap_with_log(body, &log));
    std::fs::write(&script, content.as_bytes())
        .map_err(|e| Error::io(script.display().to_string(), e))?;

    let result = elevate(&script);
    let _ = std::fs::remove_file(&script);

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let detail = std::fs::read_to_string(&log).unwrap_or_default();
            let detail = detail.trim();
            if detail.is_empty() {
                Err(e)
            } else {
                Err(Error::Other(format!("{e}\n{detail}")))
            }
        }
    }
}

fn wrap_with_log(body: &str, log: &Path) -> String {
    format!(
        r#"$log = '{log}'
try {{
{body}
    exit 0
}} catch {{
    $_ | Out-String | Set-Content -LiteralPath $log -Encoding UTF8
    exit 2
}}
"#,
        log = ps_quote(&log.display().to_string()),
        body = body
    )
}

fn elevate(script: &Path) -> Result<()> {
    let command = format!(
        "try {{ $p = Start-Process -FilePath powershell -Verb RunAs -Wait -PassThru \
         -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{script}' \
         -ErrorAction Stop }} catch {{ exit {cancelled} }}; exit $p.ExitCode",
        script = ps_quote(&script.display().to_string()),
        cancelled = ERROR_CANCELLED
    );

    let mut outer = Command::new("powershell");
    outer.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &command,
    ]);
    hide_console(&mut outer);

    let status = outer.status().map_err(|e| Error::io("powershell", e))?;

    match status.code() {
        Some(0) => Ok(()),
        Some(code) if code == ERROR_CANCELLED => Err(Error::Other(
            "the administrator rights request was declined".into(),
        )),
        Some(code) => Err(Error::Other(format!(
            "the service install script exited with code {code}"
        ))),
        None => Err(Error::Other(
            "the service install script was interrupted".into(),
        )),
    }
}

/// Escaping for single-quoted PowerShell strings: inside them there are no
/// special characters, doubling the apostrophe itself is enough.
fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_single_quotes() {
        assert_eq!(ps_quote(r"C:\Users\O'Brien\x"), r"C:\Users\O''Brien\x");
    }

    #[test]
    fn sddl_contains_user_entry() {
        let sddl = sddl_for("S-1-5-21-1-2-3-1001");
        assert!(sddl.contains("(A;;CCLCSWRPWPDTLOCRRC;;;S-1-5-21-1-2-3-1001)"));
        assert!(sddl.starts_with("D:"));
    }

    #[test]
    fn install_script_quotes_paths() {
        let script = install_script(r#""C:\p\sing-box.exe" run -c "C:\c.json""#, "S-1-5-21-9");
        assert!(script.contains(r#"$binPath = '"C:\p\sing-box.exe" run -c "C:\c.json"'"#));
        assert!(script.contains("New-Service"));
    }
}
