//! Бинарник sing-box: где он лежит, какой он версии, как его обновить.
//!
//! Два режима. Если в настройках задан `singBox.binaryPath` — это выбор
//! пользователя, мы его только читаем и в крайнем случае предупреждаем об
//! устаревшей версии. Иначе бинарником управляет Vantage Box: он лежит в
//! конфиг-директории и обновляется отсюда.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::clash::client::{compatibility, parse_version, SINGBOX_MAX_EXCLUSIVE, SINGBOX_MIN};
use crate::clash::models::Compatibility;
use crate::error::{Error, Result};
use crate::settings::{config_dir, Settings};

#[cfg(windows)]
const EXE_NAME: &str = "sing-box.exe";
#[cfg(not(windows))]
const EXE_NAME: &str = "sing-box";

/// Рабочая директория sing-box: туда лягут `cache_file` и прочее состояние.
/// Без неё сервис писал бы в системную директорию, откуда его запустили.
pub fn data_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("sing-box-data"))
}

/// Путь к активному файлу sing-box под управлением Vantage Box.
///
/// Путь стабильный: на него ссылается зарегистрированный сервис. Смена версии
/// — это подмена файла по этому пути, а не новый путь, иначе каждое
/// переключение требовало бы переустановки сервиса с UAC-запросом.
pub fn managed_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("bin").join(EXE_NAME))
}

/// Директория со скачанными версиями. Каждая лежит отдельным файлом и
/// остаётся на диске, пока её не удалят руками: откатиться на предыдущую
/// версию не должно означать «скачай её заново».
pub fn versions_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("bin").join("versions"))
}

pub fn version_path(version: &str) -> Result<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    Ok(versions_dir()?.join(format!("sing-box-{version}{suffix}")))
}

/// Версии, которые уже скачаны.
pub fn downloaded_versions() -> Vec<String> {
    let Ok(dir) = versions_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut versions: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let name = name.strip_suffix(".exe").unwrap_or(&name).to_string();
            name.strip_prefix("sing-box-").map(str::to_string)
        })
        // Недокачанный архив или чужой файл рядом не должен превратиться в
        // «версию», которую UI предложит выбрать.
        .filter(|version| parse_version(version).is_some())
        .collect();

    versions.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
    versions
}

/// Ключ для сортировки версий по убыванию: строковое сравнение поставило бы
/// 1.9.0 выше 1.11.0.
fn version_key(version: &str) -> (u32, u32, u32) {
    parse_version(version).unwrap_or((0, 0, 0))
}

/// Какой бинарник использовать по текущим настройкам.
pub fn resolve(settings: &Settings) -> Result<BinaryChoice> {
    let custom = settings.sing_box.binary_path.trim();
    if custom.is_empty() {
        Ok(BinaryChoice {
            path: managed_path()?,
            managed: true,
        })
    } else {
        Ok(BinaryChoice {
            path: PathBuf::from(custom),
            managed: false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BinaryChoice {
    pub path: PathBuf,
    /// `true` — бинарник наш, можно обновлять автоматически.
    pub managed: bool,
}

/// Сведения о бинарнике для UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryInfo {
    pub path: String,
    pub managed: bool,
    pub present: bool,
    pub version: Option<String>,
    pub compatibility: Compatibility,
    /// Почему не удалось определить версию, если не удалось.
    pub problem: Option<String>,
    /// Поддерживаемый диапазон — показываем рядом с предупреждением.
    pub supported_range: String,
}

pub fn info(settings: &Settings) -> Result<BinaryInfo> {
    let choice = resolve(settings)?;
    let present = choice.path.is_file();

    let (version, problem) = if present {
        match detect_version(&choice.path) {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        }
    } else {
        (None, None)
    };

    Ok(BinaryInfo {
        path: choice.path.display().to_string(),
        managed: choice.managed,
        present,
        compatibility: version
            .as_deref()
            .map(compatibility)
            .unwrap_or(Compatibility::Unknown),
        version,
        problem,
        supported_range: supported_range(),
    })
}

pub fn supported_range() -> String {
    let (a, b, c) = SINGBOX_MIN;
    let (x, y, z) = SINGBOX_MAX_EXCLUSIVE;
    format!(">= {a}.{b}.{c}, < {x}.{y}.{z}")
}

/// `sing-box version` печатает несколько строк; версия — в первой,
/// вида `sing-box version 1.11.4`.
pub fn detect_version(path: &Path) -> Result<String> {
    let output = run(path, &["version"])?;
    output
        .split_whitespace()
        .find_map(|token| parse_version(token).map(|_| token.trim_start_matches('v').to_string()))
        .ok_or_else(|| Error::Other(format!("не удалось разобрать вывод `sing-box version`: {output}")))
}

/// `sing-box check -c <config>` — синтаксическая и семантическая проверка.
/// Отсутствие бинарника не ошибка: тогда проверка просто недоступна, и об
/// этом надо сказать прямо, а не выдавать это за проваленную валидацию.
pub fn check_config(binary: &Path, config: &Path) -> Result<CheckResult> {
    if !binary.is_file() {
        return Ok(CheckResult {
            available: false,
            ok: false,
            output: format!(
                "файл sing-box не найден ({}), поэтому проверена только синтаксическая корректность JSON",
                binary.display()
            ),
        });
    }

    let config = config.display().to_string();
    match run(binary, &["check", "-c", &config]) {
        Ok(_) => Ok(CheckResult {
            available: true,
            ok: true,
            output: String::new(),
        }),
        Err(Error::Other(message)) => Ok(CheckResult {
            available: true,
            ok: false,
            output: message,
        }),
        Err(other) => Err(other),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    /// Была ли проверка вообще выполнена.
    pub available: bool,
    pub ok: bool,
    pub output: String,
}

/// Запускает бинарник и отдаёт stdout+stderr. Ненулевой код возврата — это
/// `Error::Other` с текстом вывода: для `check` он и есть сообщение об ошибке.
fn run(path: &Path, args: &[&str]) -> Result<String> {
    if !path.is_file() {
        return Err(Error::Other(format!(
            "файл sing-box не найден: {}",
            path.display()
        )));
    }

    let mut command = Command::new(path);
    command.args(args);
    hide_console(&mut command);

    let output = command
        .output()
        .map_err(|e| Error::io(path.display().to_string(), e))?;

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    if output.status.success() {
        Ok(text.trim().to_string())
    } else {
        Err(Error::Other(text.trim().to_string()))
    }
}

// ---------------------------------------------------------------------------
// Релизы на GitHub
// ---------------------------------------------------------------------------

const RELEASES_URL: &str = "https://api.github.com/repos/SagerNet/sing-box/releases";
/// GitHub API отклоняет запросы без User-Agent.
const USER_AGENT: &str = "vantage-box";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// Версия без префикса `v`.
    pub version: String,
    pub prerelease: bool,
    /// Попадает ли в протестированный диапазон.
    pub compatibility: Compatibility,
    /// Имя ассета под текущую платформу. `None` — сборки под неё нет.
    pub asset: Option<String>,
    pub asset_url: Option<String>,
    pub size: u64,
    /// Файл этой версии уже лежит на диске. Считается заново при каждом
    /// чтении каталога, в кэше не хранится.
    #[serde(skip_deserializing)]
    pub downloaded: bool,
    /// Именно эта версия сейчас используется.
    #[serde(skip_deserializing)]
    pub active: bool,
}

impl Default for ReleaseInfo {
    fn default() -> Self {
        Self {
            version: String::new(),
            prerelease: false,
            compatibility: Compatibility::Unknown,
            asset: None,
            asset_url: None,
            size: 0,
            downloaded: false,
            active: false,
        }
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize, Clone)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Сколько релизов просим за один запрос (максимум, разрешённый GitHub).
const RELEASES_PER_PAGE: usize = 100;
/// Потолок на число страниц — чтобы не уткнуться в лимит запросов к API.
const MAX_RELEASE_PAGES: usize = 5;

/// Список последних релизов sing-box. Предрелизы отсеиваем: обновляться на
/// beta без явного запроса — не то, чего ждёт пользователь.
///
/// Ходим по страницам, пока не наберём `limit` стабильных релизов: между
/// стабильными версиями бывают десятки alpha и beta, и на одной странице
/// нужного количества может просто не оказаться.
pub async fn fetch_releases(limit: usize) -> Result<Vec<ReleaseInfo>> {
    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    let mut collected = Vec::new();

    for page in 1..=MAX_RELEASE_PAGES {
        let response = http
            .get(RELEASES_URL)
            .query(&[
                ("per_page", RELEASES_PER_PAGE.to_string()),
                ("page", page.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::Other(format!(
                "GitHub ответил {} при запросе списка релизов",
                response.status()
            )));
        }

        let releases: Vec<GithubRelease> = response.json().await?;
        let exhausted = releases.len() < RELEASES_PER_PAGE;

        collected.extend(releases.into_iter().filter(|r| !r.prerelease).map(to_info));

        if collected.len() >= limit || exhausted {
            break;
        }
    }

    collected.truncate(limit);
    Ok(collected)
}

fn to_info(release: GithubRelease) -> ReleaseInfo {
    let version = release.tag_name.trim_start_matches('v').to_string();
    let asset = asset_name(&version)
        .as_deref()
        .and_then(|name| release.assets.iter().find(|a| a.name == name).cloned());

    ReleaseInfo {
        compatibility: compatibility(&version),
        prerelease: release.prerelease,
        asset: asset.as_ref().map(|a| a.name.clone()),
        asset_url: asset.as_ref().map(|a| a.browser_download_url.clone()),
        size: asset.map(|a| a.size).unwrap_or(0),
        version,
        ..ReleaseInfo::default()
    }
}

// ---------------------------------------------------------------------------
// Кэш каталога релизов
//
// Список релизов нужен часто (проверка обновлений, выбор версии), а меняется
// он раз в несколько недель. Поэтому в UI он всегда приезжает из кэша, а поход
// на GitHub — это либо фоновое обновление на старте, либо явная кнопка.
// ---------------------------------------------------------------------------

const CATALOG_FILE: &str = "releases.json";
/// Сколько кэш считается свежим. Дольше — обновляем в фоне при старте.
const CATALOG_TTL_SECS: u64 = 12 * 60 * 60;
/// Сколько стабильных релизов держим в каталоге.
pub const CATALOG_SIZE: usize = 15;

/// Каталог релизов в том виде, в котором его видит UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCatalog {
    /// Когда список забирали с GitHub, unix-время в секундах. 0 — никогда.
    pub fetched_at: u64,
    /// Кэш пора обновить.
    pub stale: bool,
    pub releases: Vec<ReleaseInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CachedCatalog {
    fetched_at: u64,
    releases: Vec<ReleaseInfo>,
}

fn catalog_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(CATALOG_FILE))
}

fn read_cache() -> CachedCatalog {
    let Ok(path) = catalog_path() else {
        return CachedCatalog::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_cache(cache: &CachedCatalog) -> Result<()> {
    let path = catalog_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let body = serde_json::to_vec_pretty(cache).unwrap_or_default();
    std::fs::write(&path, body).map_err(|e| Error::io(path.display().to_string(), e))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Каталог из кэша. Сеть не трогает.
pub fn cached_catalog(active_version: Option<&str>) -> ReleaseCatalog {
    finish(read_cache(), active_version)
}

/// Обновляет кэш из GitHub и отдаёт свежий каталог.
pub async fn refresh_catalog(active_version: Option<&str>) -> Result<ReleaseCatalog> {
    let releases = fetch_releases(CATALOG_SIZE).await?;
    let cache = CachedCatalog {
        fetched_at: now_secs(),
        releases,
    };
    write_cache(&cache)?;
    Ok(finish(cache, active_version))
}

/// Пора ли обновить кэш в фоне.
pub fn catalog_is_stale() -> bool {
    let cache = read_cache();
    cache.releases.is_empty() || now_secs().saturating_sub(cache.fetched_at) > CATALOG_TTL_SECS
}

/// Достраивает каталог локальными фактами: что скачано и что активно.
///
/// Скачанные версии, которых нет в списке с GitHub (например, релиз уехал из
/// первых `CATALOG_SIZE`), всё равно попадают в каталог — иначе установленную
/// версию стало бы нечем удалить.
fn finish(cache: CachedCatalog, active_version: Option<&str>) -> ReleaseCatalog {
    let downloaded = downloaded_versions();
    let mut releases = cache.releases;

    for release in &mut releases {
        release.downloaded = downloaded.iter().any(|v| v == &release.version);
        release.active = active_version == Some(release.version.as_str());
    }

    for version in downloaded {
        if releases.iter().any(|r| r.version == version) {
            continue;
        }
        releases.push(ReleaseInfo {
            compatibility: compatibility(&version),
            active: active_version == Some(version.as_str()),
            downloaded: true,
            asset: asset_name(&version),
            version,
            ..ReleaseInfo::default()
        });
    }

    releases.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));

    ReleaseCatalog {
        stale: releases.is_empty()
            || now_secs().saturating_sub(cache.fetched_at) > CATALOG_TTL_SECS,
        fetched_at: cache.fetched_at,
        releases,
    }
}

/// Имя ассета под текущую ОС и архитектуру.
///
/// Пока умеем только Windows: там ассеты — zip, и распаковщик у нас zip'овый.
/// Linux и macOS отдают tar.gz, их поддержка приедет вместе с M3.
pub fn asset_name(version: &str) -> Option<String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        _ => return None,
    };

    if cfg!(windows) {
        Some(format!("sing-box-{version}-windows-{arch}.zip"))
    } else {
        None
    }
}

/// Скачивает ассет, считая sha256 на лету.
///
/// Апстрим не публикует контрольные суммы, поэтому сверять хеш не с чем:
/// целостность обеспечивает TLS до GitHub. Посчитанное значение показываем
/// пользователю — его можно сверить вручную.
pub async fn download(url: &str, dest: &Path) -> Result<String> {
    use futures_util::StreamExt;
    use sha2::{Digest, Sha256};

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }

    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        return Err(Error::Other(format!(
            "не удалось скачать {url}: {}",
            response.status()
        )));
    }

    let mut file = std::fs::File::create(dest)
        .map_err(|e| Error::io(dest.display().to_string(), e))?;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| Error::io(dest.display().to_string(), e))?;
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Удаляет скачанную версию. Отсутствие файла — не ошибка.
pub fn remove_version(version: &str) -> Result<()> {
    let path = version_path(version)?;
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Достаёт бинарник sing-box из скачанного архива. Внутри он лежит в папке
/// вида `sing-box-1.13.16-windows-amd64/`, поэтому ищем по имени файла.
pub fn extract(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| Error::io(archive_path.display().to_string(), e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::Other(format!("не удалось прочитать архив: {e}")))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| Error::Other(format!("не удалось прочитать запись архива: {e}")))?;

        let is_target = Path::new(entry.name())
            .file_name()
            .is_some_and(|name| name == EXE_NAME);
        if !is_target || !entry.is_file() {
            continue;
        }

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        let mut out = std::fs::File::create(dest)
            .map_err(|e| Error::io(dest.display().to_string(), e))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| Error::io(dest.display().to_string(), e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755));
        }

        return Ok(());
    }

    Err(Error::Other(format!(
        "в архиве нет файла {EXE_NAME}"
    )))
}

/// На Windows дочерний процесс иначе мигал бы окном консоли.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}
