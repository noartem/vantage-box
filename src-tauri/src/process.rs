//! Запуск sing-box дочерним процессом — режим без установки сервиса.
//!
//! Сервис нужен только там, где sing-box требует прав администратора: TUN
//! поднимает сетевой адаптер, и без elevation это не работает. Конфигурация
//! без TUN (обычный http/socks-inbound) прекрасно живёт обычным процессом от
//! имени пользователя — заставлять ради неё ставить сервис незачем.
//!
//! Процесс принадлежит приложению: он останавливается при выходе, иначе после
//! закрытия GUI остался бы висеть безымянный sing-box, которым уже нечем
//! управлять.

use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::binary;
use crate::error::{Error, Result};
use crate::runtime;
use crate::settings::Settings;

/// Запущенный нами sing-box. `None` — процесс не запускался или уже завершился.
fn slot() -> &'static Mutex<Option<Child>> {
    static SLOT: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Куда уходит вывод sing-box. Через Clash API приходят те же логи, но если
/// процесс падает на старте, API подняться не успевает — и единственная
/// причина отказа видна только здесь.
pub fn log_path() -> Result<std::path::PathBuf> {
    Ok(binary::data_dir()?.join("sing-box.log"))
}

/// Работает ли наш дочерний процесс прямо сейчас.
///
/// Заодно подчищает слот: завершившийся процесс без `wait` остаётся зомби.
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
        // Состояние процесса неизвестно — считаем, что его нет: иначе кнопка
        // «Запустить» осталась бы заблокированной навсегда.
        Err(_) => {
            *guard = None;
            false
        }
    }
}

/// PID запущенного процесса — показываем его в UI как доказательство, что
/// «работает» относится к чему-то конкретному.
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

/// Запускает sing-box с уже подготовленным рантайм-конфигом.
pub fn start(settings: &Settings) -> Result<()> {
    if running() {
        return Ok(());
    }

    let choice = binary::resolve(settings)?;
    if !choice.path.is_file() {
        return Err(Error::Other(format!(
            "файл sing-box не найден: {}",
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

    // Процесс может отвалиться сразу — например, конфиг не принят. Молча
    // отрапортовать «запущено» в этом случае было бы враньём.
    std::thread::sleep(std::time::Duration::from_millis(600));
    if !running() {
        let detail = std::fs::read_to_string(&log).unwrap_or_default();
        let detail = tail(&detail, 12);
        return Err(Error::Other(if detail.is_empty() {
            "sing-box завершился сразу после запуска".into()
        } else {
            format!("sing-box завершился сразу после запуска:\n{detail}")
        }));
    }

    Ok(())
}

/// Останавливает процесс. Отсутствие процесса — не ошибка.
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
        .map_err(|e| Error::Other(format!("не удалось остановить sing-box: {e}")))?;
    let _ = child.wait();
    Ok(())
}

/// Последние `lines` строк — вывод sing-box на старте бывает длинным, а
/// причина отказа всегда в конце.
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
