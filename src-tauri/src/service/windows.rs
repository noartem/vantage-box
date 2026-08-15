//! Windows Service Control Manager.
//!
//! Установка идёт через elevated PowerShell-скрипт: `New-Service` создаёт
//! сервис, а `sc.exe sdset` дописывает в его дескриптор безопасности право
//! текущего пользователя на start/stop. После этого управление сервисом из
//! GUI прав администратора не требует.

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

/// `ERROR_SERVICE_DOES_NOT_EXIST` — единственный «нормальный» отказ SCM.
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
/// `ERROR_CANCELLED` — пользователь закрыл окно UAC.
const ERROR_CANCELLED: i32 = 1223;

/// Достаёт из ошибки `windows_service` код ОС. Её `Display` печатает безликое
/// «IO error in winapi call» и съедает внутреннюю `io::Error` — без кодa
/// («не удалось запустить сервис: IO error in winapi call») причину не найти.
fn winerr(e: &windows_service::Error) -> String {
    match e {
        windows_service::Error::Winapi(io) => {
            format!("ошибка winapi (код {}): {io}", io.raw_os_error().unwrap_or(-1))
        }
        other => other.to_string(),
    }
}

/// Сколько ждём перехода сервиса в целевое состояние.
const TRANSITION_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

// ---------------------------------------------------------------------------
// Чтение состояния
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
        .map_err(|e| Error::Other(format!("не удалось получить состояние сервиса: {}", winerr(&e))))?;

    // Права проверяем отдельным открытием: если sdset не отработал, старт
    // упрётся в отказ доступа, и об этом лучше сказать заранее.
    let can_control = open(ServiceAccess::START | ServiceAccess::STOP)
        .map(|s| s.is_some())
        .unwrap_or(false);

    Ok(ServiceInfo {
        name: SERVICE_NAME.into(),
        supported: true,
        state: map_state(status.current_state),
        can_control,
        detail: (!can_control).then(|| {
            "нет прав на управление сервисом — переустановите его, чтобы выдать их".to_string()
        }),
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

/// `Ok(None)` — сервис не зарегистрирован. Всё остальное — настоящая ошибка.
fn open(access: ServiceAccess) -> Result<Option<windows_service::service::Service>> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| Error::Other(format!("нет доступа к диспетчеру сервисов: {}", winerr(&e))))?;

    match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => Ok(Some(service)),
        Err(windows_service::Error::Winapi(io))
            if io.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST) =>
        {
            Ok(None)
        }
        Err(e) => Err(Error::Other(format!("не удалось открыть сервис: {}", winerr(&e)))),
    }
}

// ---------------------------------------------------------------------------
// Старт и остановка (без прав администратора)
// ---------------------------------------------------------------------------

pub fn start() -> Result<()> {
    let service = open(ServiceAccess::START | ServiceAccess::QUERY_STATUS)?
        .ok_or_else(|| Error::Other("сервис не установлен".into()))?;

    let current = service
        .query_status()
        .map_err(|e| Error::Other(format!("не удалось получить состояние сервиса: {}", winerr(&e))))?;
    if current.current_state == WinState::Running {
        return Ok(());
    }

    service
        .start(&[] as &[&OsStr])
        .map_err(|e| Error::Other(format!("не удалось запустить сервис: {}", winerr(&e))))?;

    wait_for(&service, WinState::Running)
}

pub fn stop() -> Result<()> {
    let service = open(ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?
        .ok_or_else(|| Error::Other("сервис не установлен".into()))?;

    let current = service
        .query_status()
        .map_err(|e| Error::Other(format!("не удалось получить состояние сервиса: {}", winerr(&e))))?;
    if current.current_state == WinState::Stopped {
        return Ok(());
    }

    service
        .stop()
        .map_err(|e| Error::Other(format!("не удалось остановить сервис: {}", winerr(&e))))?;

    wait_for(&service, WinState::Stopped)
}

/// SCM отвечает на start/stop сразу, а состояние меняется асинхронно.
fn wait_for(service: &windows_service::service::Service, target: WinState) -> Result<()> {
    let deadline = Instant::now() + TRANSITION_TIMEOUT;
    loop {
        let status = service
            .query_status()
            .map_err(|e| Error::Other(format!("не удалось получить состояние сервиса: {}", winerr(&e))))?;

        if status.current_state == target {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(Error::Other(format!(
                "сервис не перешёл в состояние {target:?} за {} с — смотрите логи sing-box",
                TRANSITION_TIMEOUT.as_secs()
            )));
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// Установка и удаление (один UAC-запрос)
// ---------------------------------------------------------------------------

pub fn install(settings: &Settings) -> Result<()> {
    let choice = binary::resolve(settings)?;
    if !choice.path.is_file() {
        return Err(Error::Other(format!(
            "файл sing-box не найден: {}",
            choice.path.display()
        )));
    }

    // Рантайм-конфиг готовим заранее: сервис будет ссылаться прямо на него.
    let prepared = runtime::prepare(settings)?;
    let data_dir = binary::data_dir()?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| Error::io(data_dir.display().to_string(), e))?;

    // Сервисом регистрируем не sing-box (он не умеет отвечать SCM — это даёт
    // ошибку 1053), а нас самих с флагом `--scm`: обёртка докладывает SCM о
    // состоянии и уже внутри поднимает sing-box ребёнком. Пути к sing-box,
    // рантайм-конфигу и data-dir передаём аргументами.
    let wrapper = std::env::current_exe()
        .map_err(|e| Error::Other(format!("не определить путь к исполняемому файлу: {e}")))?;
    if !wrapper.is_file() {
        return Err(Error::Other(format!(
            "исполняемый файл Vantage Box не найден: {}",
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
    // Пересоздаём сервис целиком: Set-Service в Windows PowerShell 5.1 не умеет
    // менять BinaryPathName, а нам нужно уметь переустановить с новыми путями.
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

/// Дескриптор безопасности сервиса: стандартные записи плюс наша — она даёт
/// конкретному пользователю право запускать (RP), останавливать (WP) и
/// опрашивать (LO, CC, LC) именно этот сервис.
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

/// SID текущего пользователя. Вывод API — чистый ASCII, локаль не мешает.
fn current_user_sid() -> Result<String> {
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-Command",
        "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
    ]);
    hide_console(&mut command);

    let output = command
        .output()
        .map_err(|e| Error::io("powershell", e))?;

    let sid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !sid.starts_with("S-1-") {
        return Err(Error::Other(
            "не удалось определить SID текущего пользователя".into(),
        ));
    }
    Ok(sid)
}

/// Пишет скрипт на диск и запускает его с повышением прав. Один UAC-запрос
/// на весь скрипт — сколько бы команд внутри ни было.
fn run_elevated(kind: &str, body: &str) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;

    let script = dir.join(format!("service-{kind}.ps1"));
    let log = dir.join(format!("service-{kind}.log"));
    let _ = std::fs::remove_file(&log);

    // BOM обязателен: Windows PowerShell 5.1 без него читает файл в ANSI и
    // ломает пути с кириллицей в имени пользователя.
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
    outer.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &command]);
    hide_console(&mut outer);

    let status = outer
        .status()
        .map_err(|e| Error::io("powershell", e))?;

    match status.code() {
        Some(0) => Ok(()),
        Some(code) if code == ERROR_CANCELLED => Err(Error::Other(
            "запрос прав администратора отклонён".into(),
        )),
        Some(code) => Err(Error::Other(format!(
            "скрипт установки сервиса завершился с кодом {code}"
        ))),
        None => Err(Error::Other("скрипт установки сервиса был прерван".into())),
    }
}

/// Экранирование для одинарных кавычек PowerShell: внутри них спецсимволов
/// нет, достаточно удвоить сам апостроф.
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
