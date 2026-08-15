//! Команды, доступные фронтенду через `invoke`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::actions::{self, blocking, RestartOutcome, RunStatus};
use crate::binary::{self, BinaryInfo, CheckResult, ReleaseCatalog};
use crate::clash::models::{ConnectionStatus, ConnectionsSnapshot, Proxy};
use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;
use crate::process;
use crate::runtime;
use crate::service;
use crate::settings::{config_dir, Settings};
use crate::state::{self, AppState};
use crate::subscription::{self, ApplyOutcome, SubscriptionsState};

// ---------------------------------------------------------------------------
// Настройки
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.get()
}

#[tauri::command]
pub fn get_settings_path(state: State<'_, AppState>) -> String {
    state.settings.path().display().to_string()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings> {
    let previous = state.settings.get();
    state.settings.save(settings.clone())?;
    state::apply_settings(&app, &state, &previous, &settings)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Подключение
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> ConnectionStatus {
    state.streams.status()
}

// ---------------------------------------------------------------------------
// Прокси
// ---------------------------------------------------------------------------

/// Плоский ответ `/proxies` неудобен для UI, поэтому раскладываем его на
/// группы с уже подставленными узлами и последними замерами задержки.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyOverview {
    pub groups: Vec<GroupView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub name: String,
    /// `Selector`, `URLTest`, …
    pub kind: String,
    pub now: Option<String>,
    /// Можно ли переключать выбор руками.
    pub selectable: bool,
    pub items: Vec<NodeView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub name: String,
    pub kind: String,
    /// Последний известный замер, мс. `None` — не измеряли или узел не ответил.
    pub delay: Option<u32>,
    pub udp: bool,
    /// Вложенная группа: клик по ней должен вести внутрь, а не только выбирать.
    pub is_group: bool,
}

#[tauri::command]
pub async fn get_proxies(state: State<'_, AppState>) -> Result<ProxyOverview> {
    let client = state.client();
    let response = client.proxies().await?;
    Ok(build_overview(response.proxies))
}

#[tauri::command]
pub async fn select_proxy(app: AppHandle, group: String, name: String) -> Result<()> {
    actions::select_proxy(&app, &group, &name).await
}

#[tauri::command]
pub async fn test_group_delay(
    state: State<'_, AppState>,
    group: String,
) -> Result<HashMap<String, u32>> {
    let settings = state.settings.get();
    let client = state.client();
    client
        .group_delay(
            &group,
            &settings.ui.latency_test_url,
            settings.ui.latency_test_timeout,
        )
        .await
}

#[tauri::command]
pub async fn test_proxy_delay(state: State<'_, AppState>, name: String) -> Result<u32> {
    let settings = state.settings.get();
    let client = state.client();
    client
        .proxy_delay(
            &name,
            &settings.ui.latency_test_url,
            settings.ui.latency_test_timeout,
        )
        .await
}

// ---------------------------------------------------------------------------
// Соединения
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_connections(state: State<'_, AppState>) -> Result<ConnectionsSnapshot> {
    let client = state.client();
    client.connections().await
}

#[tauri::command]
pub async fn close_connection(state: State<'_, AppState>, id: String) -> Result<()> {
    let client = state.client();
    client.close_connection(&id).await
}

#[tauri::command]
pub async fn close_all_connections(state: State<'_, AppState>) -> Result<()> {
    let client = state.client();
    client.close_all_connections().await
}

// ---------------------------------------------------------------------------
// Конфиг sing-box
// ---------------------------------------------------------------------------

/// Читает пользовательский `config.json` для встроенного редактора.
#[tauri::command]
pub fn read_singbox_config(state: State<'_, AppState>) -> Result<String> {
    let path = config_path(&state)?;
    let content = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
    // Считаем прочитанное «известным»: сразу после открытия редактора файл
    // не должен выглядеть изменённым снаружи.
    state.remember_config(&content);
    Ok(content)
}

/// Проверяет содержимое редактора, не трогая пользовательский файл.
///
/// Сначала JSON, затем — если бинарник доступен — полноценный `sing-box check`
/// на временной копии.
#[tauri::command]
pub async fn check_singbox_config(
    state: State<'_, AppState>,
    content: String,
) -> Result<CheckResult> {
    let settings = state.settings.get();
    blocking(move || {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&strip_jsonc(&content)) {
            return Ok(CheckResult {
                available: false,
                ok: false,
                output: format!("некорректный JSON: {e}"),
            });
        }

        let scratch = config_dir()?.join("check.json");
        if let Some(parent) = scratch.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        std::fs::write(&scratch, content.as_bytes())
            .map_err(|e| Error::io(scratch.display().to_string(), e))?;

        let choice = binary::resolve(&settings)?;
        let mut result = binary::check_config(&choice.path, &scratch)?;
        let _ = std::fs::remove_file(&scratch);

        if !result.available {
            // JSON уже разобран выше, так что синтаксически всё в порядке.
            result.ok = true;
        }
        Ok(result)
    })
    .await
}

/// Записывает содержимое редактора в пользовательский `config.json`.
/// Предыдущая версия остаётся рядом как `.bak`.
#[tauri::command]
pub async fn write_singbox_config(state: State<'_, AppState>, content: String) -> Result<()> {
    let path = PathBuf::from(config_path(&state)?);
    let body = content.clone();

    blocking(move || {
        if path.is_file() {
            let backup = path.with_extension("json.bak");
            std::fs::copy(&path, &backup)
                .map_err(|e| Error::io(backup.display().to_string(), e))?;
        }

        // Атомарно: sing-box может читать файл в этот же момент.
        let tmp = path.with_extension("json.vbtmp");
        std::fs::write(&tmp, body.as_bytes())
            .map_err(|e| Error::io(tmp.display().to_string(), e))?;
        std::fs::rename(&tmp, &path).map_err(|e| Error::io(path.display().to_string(), e))?;
        Ok(())
    })
    .await?;

    state.remember_config(&content);
    Ok(())
}

/// Создаёт минимальный рабочий `config.json` в каталоге приложения и возвращает
/// путь к нему — для онбординга первого запуска. Конфиг без TUN (не нужны права
/// администратора): локальный mixed-инбаунд, `direct`/`block` и selector `proxy`.
/// `experimental.clash_api` сюда не пишем — его дописывает рантайм-копия.
///
/// Если файл уже существует по пути по умолчанию, не затираем его — отдаём путь.
#[tauri::command]
pub fn create_minimal_config() -> Result<String> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let path = dir.join("config.json");
    if path.is_file() {
        return Ok(path.display().to_string());
    }

    let minimal = r#"{
  "log": { "level": "info", "timestamp": true },
  "inbounds": [
    { "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": 2080 }
  ],
  "outbounds": [
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" },
    { "type": "selector", "tag": "proxy", "outbounds": ["direct"], "default": "direct" }
  ],
  "route": { "final": "proxy" }
}
"#;

    let tmp = dir.join("config.json.vbtmp");
    std::fs::write(&tmp, minimal.as_bytes())
        .map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(path.display().to_string())
}

fn config_path(state: &State<'_, AppState>) -> Result<String> {
    let path = state.settings.get().sing_box.config_path;
    if path.trim().is_empty() {
        return Err(Error::Other(
            "путь к config.json не задан — укажите его в настройках".into(),
        ));
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// Сервис sing-box
//
// Тела операций живут в `actions`: то же самое умеет делать трей, и поведение
// не должно расходиться между окном и меню в трее.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_run_status(app: AppHandle) -> Result<RunStatus> {
    actions::run_status(&app).await
}

/// Регистрирует сервис. Единственное место, где всплывает UAC.
#[tauri::command]
pub async fn install_service(app: AppHandle) -> Result<RunStatus> {
    actions::install(&app).await
}

#[tauri::command]
pub async fn uninstall_service(app: AppHandle) -> Result<RunStatus> {
    actions::uninstall(&app).await
}

#[tauri::command]
pub async fn start_service(app: AppHandle) -> Result<RunStatus> {
    actions::start(&app).await
}

#[tauri::command]
pub async fn stop_service(app: AppHandle) -> Result<RunStatus> {
    actions::stop(&app).await
}

#[tauri::command]
pub async fn restart_service(app: AppHandle) -> Result<RestartOutcome> {
    actions::restart(&app).await
}

// ---------------------------------------------------------------------------
// Окна и хоткеи
// ---------------------------------------------------------------------------

/// Хоткеи, которые не удалось занять: комбинация может быть занята другой
/// программой или записана с опечаткой.
#[tauri::command]
pub fn get_hotkey_problems(state: State<'_, AppState>) -> Vec<String> {
    state
        .hotkey_problems
        .lock()
        .expect("hotkey problems lock")
        .clone()
}

/// Попап закрывает сам себя — например, сразу после выбора узла.
#[tauri::command]
pub fn close_popup(app: AppHandle) {
    crate::window::hide_popup(&app);
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    crate::window::show_main(&app);
}

// ---------------------------------------------------------------------------
// Помощники для формы настроек
// ---------------------------------------------------------------------------

/// Новый secret для Clash API. Генерируем здесь, а не в вебвью: источник
/// энтропии в JS слабее, а токен защищает локальный порт управления.
#[tauri::command]
pub fn generate_secret() -> String {
    runtime::generate_secret()
}

/// Системный диалог выбора файла. `kind`: `config` или `binary`.
/// `None` — пользователь закрыл диалог, это не ошибка.
#[tauri::command]
pub async fn pick_file(app: AppHandle, kind: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;

    let mut builder = app.dialog().file().set_title(if kind == "config" {
        "Выберите config sing-box"
    } else {
        "Выберите файл sing-box"
    });

    if kind == "config" {
        builder = builder.add_filter("JSON", &["json", "jsonc"]);
    } else if cfg!(windows) {
        builder = builder.add_filter("Программа", &["exe"]);
    }
    builder = builder.add_filter("Все файлы", &["*"]);

    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.pick_file(move |path| {
        let _ = tx.send(path);
    });

    rx.await
        .ok()
        .flatten()
        .and_then(|path| path.into_path().ok())
        .map(|path| path.display().to_string())
}

// ---------------------------------------------------------------------------
// Файл sing-box и каталог версий
//
// Каждая версия скачивается отдельным файлом и остаётся на диске. Активная —
// копия выбранной версии по стабильному пути: на него ссылается сервис, и
// переключение версии не должно требовать его переустановки.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_binary_info(state: State<'_, AppState>) -> Result<BinaryInfo> {
    let settings = state.settings.get();
    blocking(move || binary::info(&settings)).await
}

/// Каталог релизов. По умолчанию — из кэша, `refresh` заставляет сходить
/// на GitHub.
#[tauri::command]
pub async fn list_singbox_releases(
    state: State<'_, AppState>,
    refresh: bool,
) -> Result<ReleaseCatalog> {
    let active = active_version(&state).await;

    if !refresh {
        return blocking(move || Ok(binary::cached_catalog(active.as_deref()))).await;
    }

    binary::refresh_catalog(active.as_deref()).await
}

/// Скачивает версию в каталог, ничего не переключая.
#[tauri::command]
pub async fn download_singbox_release(
    state: State<'_, AppState>,
    version: String,
    asset_url: String,
) -> Result<ReleaseCatalog> {
    let target = binary::version_path(&version)?;
    // Имя нарочно не похоже на файл версии: недокачанный архив не должен
    // попасть в список скачанных версий.
    let archive = binary::versions_dir()?.join(format!("download-{version}.archive"));

    binary::download(&asset_url, &archive).await?;

    let archive_path = archive.clone();
    blocking(move || {
        let result = binary::extract(&archive_path, &target);
        let _ = std::fs::remove_file(&archive_path);
        result
    })
    .await?;

    let active = active_version(&state).await;
    blocking(move || Ok(binary::cached_catalog(active.as_deref()))).await
}

#[tauri::command]
pub async fn delete_singbox_release(
    state: State<'_, AppState>,
    version: String,
) -> Result<ReleaseCatalog> {
    let active = active_version(&state).await;
    if active.as_deref() == Some(version.as_str()) {
        return Err(Error::Other(
            "эта версия сейчас используется — сначала выберите другую".into(),
        ));
    }

    blocking(move || {
        binary::remove_version(&version)?;
        Ok(binary::cached_catalog(active.as_deref()))
    })
    .await
}

/// Результат переключения версии — всё, что стоит показать после него.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub binary: BinaryInfo,
    /// Пришлось ли останавливать sing-box ради замены файла.
    pub restarted: bool,
    /// Что сказал `sing-box check` новой версией на текущем конфиге.
    pub check: CheckResult,
}

/// Делает скачанную версию активной: проверить конфиг ею → остановить
/// sing-box → заменить файл → запустить обратно.
///
/// Работает только с файлом под управлением Vantage Box: чужой путь — это
/// выбор пользователя, туда мы не пишем.
#[tauri::command]
pub async fn use_singbox_release(
    state: State<'_, AppState>,
    version: String,
) -> Result<InstallOutcome> {
    let settings = state.settings.get();
    let choice = binary::resolve(&settings)?;
    if !choice.managed {
        return Err(Error::Other(
            "путь к файлу sing-box задан вручную — Vantage Box его не подменяет".into(),
        ));
    }

    let source = binary::version_path(&version)?;
    if !source.is_file() {
        return Err(Error::Other(format!(
            "версия {version} не скачана: {}",
            source.display()
        )));
    }

    let target = choice.path.clone();
    let job_settings = settings.clone();

    let (check, restarted) = blocking(move || {
        // Проверяем конфиг именно новой версией: несовместимые опции должны
        // всплыть до того, как мы заменим рабочий файл.
        let config = job_settings.sing_box.config_path.trim().to_string();
        let check = if config.is_empty() {
            CheckResult {
                available: false,
                ok: true,
                output: "путь к config не задан, проверка пропущена".into(),
            }
        } else {
            binary::check_config(&source, std::path::Path::new(&config))?
        };

        if !check.ok {
            return Err(Error::Other(format!(
                "версия {version} не принимает текущий config, переключение отменено:\n{}",
                check.output
            )));
        }

        let was_running = process::running()
            || service::status().map(|info| info.is_running()).unwrap_or(false);
        if was_running {
            process::stop()?;
            if service::status()
                .map(|info| info.state != crate::service::ServiceState::NotInstalled)
                .unwrap_or(false)
            {
                service::stop()?;
            }
        }

        replace_binary(&source, &target)?;

        if was_running {
            if service::status()
                .map(|info| info.state != crate::service::ServiceState::NotInstalled)
                .unwrap_or(false)
            {
                runtime::prepare(&job_settings)?;
                service::start()?;
            } else {
                process::start(&job_settings)?;
            }
        }

        Ok((check, was_running))
    })
    .await?;

    if restarted {
        state::reconnect(&state)?;
    }

    let info_settings = state.settings.get();
    let binary = blocking(move || binary::info(&info_settings)).await?;

    Ok(InstallOutcome {
        binary,
        restarted,
        check,
    })
}

/// Версия активного файла sing-box. `None` — файла нет или версия не читается.
async fn active_version(state: &State<'_, AppState>) -> Option<String> {
    let settings = state.settings.get();
    blocking(move || binary::info(&settings))
        .await
        .ok()
        .and_then(|info| info.version)
}

/// Ставит файл активным, уводя предыдущий в сторону: на Windows нельзя
/// перезаписать исполняемый файл, который ещё кем-то удерживается, зато
/// переименовать — можно.
///
/// Источник копируется, а не переносится: это файл из каталога версий, он
/// должен остаться на диске.
fn replace_binary(source: &std::path::Path, target: &std::path::Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }

    let previous = target.with_extension("old");
    let _ = std::fs::remove_file(&previous);

    if target.exists() {
        std::fs::rename(target, &previous)
            .map_err(|e| Error::io(target.display().to_string(), e))?;
    }

    match std::fs::copy(source, target) {
        Ok(_) => {
            let _ = std::fs::remove_file(&previous);
            Ok(())
        }
        Err(e) => {
            // Откатываемся, чтобы не остаться вообще без файла sing-box.
            if previous.exists() {
                let _ = std::fs::rename(&previous, target);
            }
            Err(Error::io(target.display().to_string(), e))
        }
    }
}

fn build_overview(proxies: HashMap<String, Proxy>) -> ProxyOverview {
    // Порядок групп берём из GLOBAL, если sing-box его отдал: он отражает
    // порядок outbound'ов в конфиге, а не случайный обход хеш-таблицы.
    let global_order: Vec<String> = proxies
        .get("GLOBAL")
        .and_then(|p| p.all.clone())
        .unwrap_or_default();

    // Имя берём из ключа карты: поле `name` внутри объекта sing-box отдаёт
    // не всегда, а ключ есть всегда.
    let mut groups: Vec<GroupView> = proxies
        .iter()
        .filter(|(name, p)| p.is_group() && name.as_str() != "GLOBAL")
        .map(|(group_name, group)| {
            let items = group
                .all
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|name| {
                    let node = proxies.get(&name);
                    NodeView {
                        delay: node
                            .and_then(|n| n.history.last())
                            .map(|h| h.delay)
                            // Ноль означает «узел не ответил», а не мгновенный отклик.
                            .filter(|d| *d > 0),
                        kind: node.map(|n| n.kind.clone()).unwrap_or_default(),
                        udp: node.is_some_and(|n| n.udp),
                        is_group: node.is_some_and(|n| n.is_group()),
                        name,
                    }
                })
                .collect();

            GroupView {
                name: group_name.clone(),
                kind: group.kind.clone(),
                now: group.now.clone(),
                selectable: group.is_selectable(),
                items,
            }
        })
        .collect();

    // Порядок из GLOBAL, всё остальное — по алфавиту следом. Без второго
    // ключа порядок плавал бы от вызова к вызову: источник — HashMap.
    groups.sort_by_key(|g| {
        let rank = global_order
            .iter()
            .position(|n| n == &g.name)
            .unwrap_or(usize::MAX);
        (rank, g.name.clone())
    });

    ProxyOverview { groups }
}

// ---------------------------------------------------------------------------
// Подписки
// ---------------------------------------------------------------------------

/// Перетягивает все включённые подписки и вливает узлы в config.json. `force`
/// игнорирует подпись набора — например, когда пользователь нажал «обновить».
#[tauri::command]
pub async fn refresh_subscriptions(app: AppHandle, force: bool) -> Result<ApplyOutcome> {
    subscription::apply(&app, force).await
}

/// Состояние подписок из sidecar-файла: время последнего обновления, число
/// узлов, ошибки — для отображения в UI.
#[tauri::command]
pub fn get_subscription_state() -> Result<SubscriptionsState> {
    subscription::load_state()
}
