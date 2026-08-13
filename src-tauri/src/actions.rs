//! Операции над сервисом и прокси, общие для UI и трея.
//!
//! Трей и окно должны вести себя одинаково, поэтому логика живёт здесь, а
//! `commands.rs` и `tray.rs` — только тонкие обёртки над ней.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::clash::ClashClient;
use crate::error::{Error, Result};
use crate::process;
use crate::service::{self, ServiceInfo, ServiceState};
use crate::settings::Settings;
use crate::state::{self, AppState};
use crate::runtime;

/// Событие для UI: состояние sing-box изменилось (в том числе из трея).
pub const EVENT_SERVICE: &str = "service://changed";
/// Событие для UI: выбор в selector-группе изменился (в том числе из трея).
pub const EVENT_PROXIES: &str = "proxies://changed";

/// Сколько ждём Clash API после перезапуска sing-box.
const API_TIMEOUT: Duration = Duration::from_secs(20);

/// Блокирующая работа (SCM, запуск процессов) — не на потоках async-рантайма.
pub async fn blocking<T, F>(work: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| Error::Other(format!("фоновая задача прервана: {e}")))?
}

/// Как именно запускается sing-box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunMode {
    /// Через системный сервис — единственный способ поднять TUN.
    Service,
    /// Обычным дочерним процессом от имени пользователя.
    Process,
}

/// Полное состояние sing-box для UI и трея.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub mode: RunMode,
    /// Работает ли sing-box — неважно, сервисом или процессом.
    pub running: bool,
    pub service: ServiceInfo,
    /// PID дочернего процесса, если запуск идёт мимо сервиса.
    pub process_pid: Option<u32>,
    /// Конфигу нужен TUN, а значит — сервис и права администратора.
    pub tun: bool,
    /// Почему не удалось прочитать конфиг. Тогда `tun` считаем ложным.
    pub config_problem: Option<String>,
}

impl RunStatus {
    /// Можно ли запустить прямо сейчас: без сервиса TUN не поднять.
    pub fn can_start(&self) -> bool {
        self.mode == RunMode::Service || !self.tun
    }
}

pub async fn run_status(app: &AppHandle) -> Result<RunStatus> {
    let settings = app.state::<AppState>().settings.get();
    blocking(move || build_status(&settings)).await
}

fn build_status(settings: &Settings) -> Result<RunStatus> {
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
        running: service.is_running() || process_pid.is_some(),
        service,
        process_pid,
        tun,
        config_problem,
    })
}

/// Запуск: пересобираем рантайм-конфиг (secret на каждый запуск новый) и
/// сразу переподключаемся с ним.
///
/// Зарегистрированный сервис имеет приоритет: раз пользователь его поставил,
/// именно он и должен управлять sing-box. Без сервиса запускаем процессом —
/// но только если конфигу не нужен TUN.
pub async fn start(app: &AppHandle) -> Result<RunStatus> {
    let settings = app.state::<AppState>().settings.get();

    blocking(move || {
        let status = build_status(&settings)?;

        if status.mode == RunMode::Service {
            runtime::prepare(&settings)?;
            return service::start();
        }

        if status.tun {
            return Err(Error::Other(
                "конфигу нужен TUN — для него требуются права администратора. \
                 Установите сервис на вкладке «Сервис»."
                    .into(),
            ));
        }

        process::start(&settings)
    })
    .await?;

    state::reconnect(&app.state::<AppState>())?;
    announce(app).await
}

/// Останавливаем и то, и другое: сервис мог быть установлен уже после того,
/// как мы подняли процесс, — тогда «остановить» обязано убрать оба.
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
    announce(app).await
}

/// Запустить, если стоит; остановить, если работает. Для трея и хоткея.
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
    blocking(move || service::install(&settings)).await?;
    announce(app).await
}

/// Удаление сервиса не должно оставлять sing-box работающим: после него
/// управлять запущенным сервисом было бы уже нечем.
pub async fn uninstall(app: &AppHandle) -> Result<RunStatus> {
    blocking(|| {
        let service = service::status()?;
        if service.is_running() {
            let _ = service::stop();
        }
        service::uninstall()
    })
    .await?;
    announce(app).await
}

/// Что удалось сохранить при мягком перезапуске.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartOutcome {
    pub status: RunStatus,
    /// Группы, чей выбор восстановлен: `["группа → узел", …]`.
    pub restored: Vec<String>,
    /// Группы, выбор которых восстановить не вышло, с причиной.
    pub skipped: Vec<String>,
    /// Успел ли Clash API подняться до истечения ожидания.
    pub api_back: bool,
}

/// Мягкий перезапуск: снимаем текущие выборы selector'ов, перезапускаем
/// сервис и накатываем выборы обратно.
///
/// sing-box и сам умеет помнить выбор через `experimental.cache_file` — это
/// первый уровень. Восстановление поверх работает независимо от того,
/// включён ли кэш, и чинит случай, когда он выключен.
pub async fn restart(app: &AppHandle) -> Result<RestartOutcome> {
    let settings = app.state::<AppState>().settings.get();

    // Снимок делаем до остановки — после неё спрашивать будет некого.
    let snapshot = snapshot_selection(&app.state::<AppState>().client()).await;

    blocking(move || {
        let status = build_status(&settings)?;

        if status.mode == RunMode::Service {
            runtime::prepare(&settings)?;
            service::stop()?;
            return service::start();
        }

        if status.tun {
            return Err(Error::Other(
                "конфигу нужен TUN — для него требуются права администратора. \
                 Установите сервис на вкладке «Сервис»."
                    .into(),
            ));
        }

        process::stop()?;
        process::start(&settings)
    })
    .await?;

    // Secret на новом запуске другой, поэтому клиента надо пересобрать
    // раньше, чем мы начнём стучаться в API.
    state::reconnect(&app.state::<AppState>())?;

    let client = app.state::<AppState>().client();
    let api_back = wait_for_api(&client, API_TIMEOUT).await;

    let (restored, skipped) = if api_back {
        restore_selection(&client, snapshot).await
    } else {
        (
            Vec::new(),
            vec![format!(
                "Clash API не поднялся за {} с — выбор не восстановлен",
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
    app.state::<AppState>()
        .client()
        .select(group, name)
        .await?;
    let _ = app.emit(EVENT_PROXIES, ());
    Ok(())
}

/// Читает состояние sing-box и рассказывает о нём окну: действие могло прийти
/// из трея, и UI об этом иначе узнал бы только на следующем опросе.
async fn announce(app: &AppHandle) -> Result<RunStatus> {
    let info = run_status(app).await?;
    let _ = app.emit(EVENT_SERVICE, info.clone());
    Ok(info)
}

/// `группа → выбранный узел` для всех групп, которыми можно управлять руками.
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
            skipped.push(format!("не удалось прочитать /proxies: {e}"));
            return (restored, skipped);
        }
    };

    for (group, wanted) in snapshot {
        let Some(proxy) = current.get(&group) else {
            // Конфиг мог поменяться между запусками — это нормально.
            skipped.push(format!("{group}: группы больше нет"));
            continue;
        };

        if !proxy.all.as_ref().is_some_and(|all| all.contains(&wanted)) {
            skipped.push(format!("{group}: узла «{wanted}» больше нет"));
            continue;
        }

        if proxy.now.as_deref() == Some(wanted.as_str()) {
            // sing-box уже восстановил выбор из cache_file — трогать нечего.
            continue;
        }

        match client.select(&group, &wanted).await {
            Ok(()) => restored.push(format!("{group} → {wanted}")),
            Err(e) => skipped.push(format!("{group}: {e}")),
        }
    }

    (restored, skipped)
}

/// Ждём, пока sing-box поднимет Clash API после перезапуска.
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
