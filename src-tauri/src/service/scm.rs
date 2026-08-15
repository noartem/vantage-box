//! Обёртка-сервис: SCM стартует этот же бинарник с флагом `--scm`, а обёртка
//! поднимает sing-box дочерним процессом и докладывает SCM о состоянии.
//!
//! `sing-box run` — консольная программа: она не реализует протокол Service
//! Control Manager и не вызывает `SetServiceStatus`. Зарегистрировать её
//! сервисом напрямую нельзя — SCM ждёт ответа 60 секунд и выдаёт ошибку 1053
//! («service did not respond … in a timely fashion»). Поэтому сервисом
//! регистрируем себя: мы говорим с SCM, а sing-box запускаем как ребёнка и
//! проксируем ему STOP/SHUTDOWN, убивая процесс.
//!
//! Это ровно то, что делают NSSM/WinSW, но без внешней зависимости: обёртка
//! живёт в основном бинарнике и выбирается одним флагом командной строки.
//!
//! ## Поток управления
//!
//! `register` нельзя звать прямо из `main`: `RegisterServiceCtrlHandlerExW`
//! требует, чтобы поток был запущен диспетчером через
//! `StartServiceCtrlDispatcherW`, иначе она падает с 1063
//! (`ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`). Поэтому `main` вызывает
//! [`dispatch`], который подключает процесс к SCM (и блокируется до остановки
//! сервиса), а SCM уже на отдельном потоке зовёт [`service_main`] — и только
//! оттуда мы регистрируем обработчик и докладываем о статусе.

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

/// Флаг командной строки, включающий режим обёртки-сервиса.
pub const FLAG: &str = "--scm";

/// Запущен ли бинарник SCM как сервис (тогда Tauri поднимать не нужно).
pub fn is_invocation() -> bool {
    std::env::args().any(|a| a == FLAG)
}

/// Аргументы обёртки после `--scm`: путь к sing-box, рантайм-конфиг, data-dir.
pub struct ScmArgs {
    pub sing_box: PathBuf,
    pub config: PathBuf,
    pub data_dir: PathBuf,
}

/// Разобрать `--scm <sing-box> <config> <data-dir>` из командной строки.
fn parse_args() -> Result<ScmArgs> {
    let mut it = std::env::args().skip_while(|a| a != FLAG).skip(1);
    let mut nth = |label: &str| {
        it.next().ok_or_else(|| Error::Other(format!("--scm: не передан аргумент «{label}»")))
    };
    Ok(ScmArgs {
        sing_box: PathBuf::from(nth("путь к sing-box")?),
        config: PathBuf::from(nth("путь к config")?),
        data_dir: PathBuf::from(nth("data-dir")?),
    })
}

/// Пришла ли от SCM команда остановиться. Ставится обработчиком ниже.
static STOP: AtomicBool = AtomicBool::new(false);

/// Точка входа, которую SCM дёргает на отдельном потоке после того, как
/// [`dispatch`] подключил процесс к диспетчеру. Здесь мы регистрируем
/// обработчик управляющих сигналов и поднимаем sing-box. Ошибки пишем в
/// `scm.log` — stderr под SCM никуда не пишется, иначе причина пропадает.
fn service_main(_arguments: Vec<OsString>) {
    let _ = scm_log("service_main: вход");
    if let Err(e) = run() {
        let _ = scm_log(&format!("service_main: ошибка — {e}"));
        eprintln!("vantage-box scm: {e}");
    }
    let _ = scm_log("service_main: выход");
}

define_windows_service!(ffi_service_main, service_main);

/// Подключает процесс к SCM и блокирует текущий поток, пока сервис не
/// остановится. Вызывается из `main` (`lib::run`) при `--scm`. На отдельном
/// потоке SCM запускает [`service_main`].
pub fn dispatch() {
    let _ = scm_log("dispatch: подключаюсь к SCM");
    if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        // 1063 и т.п. — процесс не стартован SCM как сервис. Пишем в лог:
        // это единственный способ увидеть причину, если обёртку запустили
        // вручную или SCM не дал подключиться.
        let _ = scm_log(&format!("dispatch: не подключиться к SCM — {}", winerr(&e)));
        eprintln!("vantage-box scm dispatch: {e}");
    }
    let _ = scm_log("dispatch: возврат из диспетчера");
}

/// Регистрируемся в SCM, поднимаем sing-box ребёнком и держим сервис живым,
/// пока sing-box работает или SCM не попросит остановиться. Возвращает ошибку,
/// если sing-box упал с ненулевым кодом — SCM запишет его в состояние сервиса.
fn run() -> Result<()> {
    STOP.store(false, Ordering::SeqCst);
    let args = parse_args()?;
    let _ = scm_log(&format!(
        "run: sing-box={} config={} data={}",
        args.sing_box.display(),
        args.config.display(),
        args.data_dir.display()
    ));

    // Обработчик управляющих сигналов SCM. Захватывать ничего не нужно: флаг
    // остановки живёт в статике, поэтому замыкание implements Fn + Send.
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
        .map_err(|e| Error::Other(format!("SCM: не зарегистрировать обработчик — {}", winerr(&e))))?;

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
            .map_err(|e| Error::Other(format!("SCM: не обновить статус — {}", winerr(&e))))
    };

    report(ServiceState::StartPending, 1, Duration::from_secs(20))?;

    // Вывод sing-box — в тот же лог, что и у процессного режима: если sing-box
    // падает на старте, причина видна именно здесь, а не в API (он не успевает).
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
    let _ = scm_log("run: sing-box запущен, сервис Running");

    // Держим сервис живым, пока sing-box работает. Выходим из цикла, когда:
    //  * SCM прислал STOP — тогда сами убиваем ребёнка;
    //  * sing-box завершился сам — например, упал на старте с ошибкой конфига.
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
    let _ = scm_log(&format!("run: sing-box завершился с кодом {exit_code}"));

    if exit_code == 0 {
        Ok(())
    } else {
        Err(Error::Other(format!("sing-box завершился с кодом {exit_code}")))
    }
}

/// Достаёт из ошибки `windows_service` код ОС — по умолчанию её `Display`
/// печатает безликое «IO error in winapi call» и съедает внутреннюю `io::Error`,
/// что делает диагностику невозможной.
fn winerr(e: &windows_service::Error) -> String {
    match e {
        windows_service::Error::Winapi(io) => {
            format!("ошибка winapi (код {}): {io}", io.raw_os_error().unwrap_or(-1))
        }
        other => other.to_string(),
    }
}

/// SCM стартует процесс без консоли, поэтому `eprintln!` уходят в никуда. Этот
/// лог — единственный способ увидеть, на чём обёртка споткнулась (особенно на
/// этапе до того, как создан `sing-box.log`). Лежит рядом с настройками:
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