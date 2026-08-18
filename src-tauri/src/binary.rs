//! The sing-box binary: where it lives, what version it is, how to update it.
//!
//! Two modes. If `singBox.binaryPath` is set in settings — that is the user's
//! choice, we only read it and, if needed, warn about an outdated version.
//! Otherwise Vantage Box manages the binary: it lives in the config directory
//! and is updated from here.

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

/// The sing-box working directory: where `cache_file` and other state land.
/// Without it the service would write to the system directory it was started from.
pub fn data_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("sing-box-data"))
}

/// Path to the active sing-box file managed by Vantage Box.
///
/// The path is stable: the registered service points to it. Switching versions
/// is a file swap at this path, not a new path — otherwise every switch would
/// require reinstalling the service with a UAC prompt.
pub fn managed_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("bin").join(EXE_NAME))
}

/// Directory with downloaded versions. Each one is a separate file and stays
/// on disk until removed by hand: rolling back to a previous version must not
/// mean "download it again".
pub fn versions_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("bin").join("versions"))
}

pub fn version_path(version: &str) -> Result<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    Ok(versions_dir()?.join(format!("sing-box-{version}{suffix}")))
}

/// Versions that are already downloaded.
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
        // An unfinished download or a stray foreign file must not turn into a
        // "version" the UI would offer to pick.
        .filter(|version| parse_version(version).is_some())
        .collect();

    versions.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
    versions
}

/// Sort key for versions in descending order: a string compare would put
/// 1.9.0 above 1.11.0.
fn version_key(version: &str) -> (u32, u32, u32) {
    parse_version(version).unwrap_or((0, 0, 0))
}

/// Which binary to use given the current settings.
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
    /// `true` — the binary is ours, can be auto-updated.
    pub managed: bool,
}

/// Details about the binary for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryInfo {
    pub path: String,
    pub managed: bool,
    pub present: bool,
    pub version: Option<String>,
    pub compatibility: Compatibility,
    /// Why the version could not be detected, if it could not.
    pub problem: Option<String>,
    /// The supported range — shown next to the warning.
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

/// `sing-box version` prints several lines; the version is in the first one,
/// like `sing-box version 1.11.4`.
pub fn detect_version(path: &Path) -> Result<String> {
    let output = run(path, &["version"])?;
    output
        .split_whitespace()
        .find_map(|token| parse_version(token).map(|_| token.trim_start_matches('v').to_string()))
        .ok_or_else(|| Error::Other(format!("could not parse `sing-box version` output: {output}")))
}

/// `sing-box check -c <config>` — syntactic and semantic validation.
/// A missing binary is not an error: then the check is simply unavailable,
/// and we should say so directly rather than pass it off as a failed validation.
pub fn check_config(binary: &Path, config: &Path) -> Result<CheckResult> {
    if !binary.is_file() {
        return Ok(CheckResult {
            available: false,
            ok: false,
            output: format!(
                "sing-box file not found ({}), so only JSON syntactic validity was checked",
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
    /// Whether the check was performed at all.
    pub available: bool,
    pub ok: bool,
    pub output: String,
}

/// Runs the binary and returns stdout+stderr. A non-zero exit code is an
/// `Error::Other` with the output text: for `check` that is the error message.
fn run(path: &Path, args: &[&str]) -> Result<String> {
    if !path.is_file() {
        return Err(Error::Other(format!(
            "sing-box file not found: {}",
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
// Releases on GitHub
// ---------------------------------------------------------------------------

const RELEASES_URL: &str = "https://api.github.com/repos/SagerNet/sing-box/releases";
/// The GitHub API rejects requests without a User-Agent.
const USER_AGENT: &str = "vantage-box";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// Version without the `v` prefix.
    pub version: String,
    pub prerelease: bool,
    /// Whether it falls in the tested range.
    pub compatibility: Compatibility,
    /// Asset name for the current platform. `None` — no build for it.
    pub asset: Option<String>,
    pub asset_url: Option<String>,
    pub size: u64,
    /// The file for this version is already on disk. Recomputed on every
    /// catalog read, not stored in the cache.
    #[serde(skip_deserializing)]
    pub downloaded: bool,
    /// This exact version is currently in use.
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

/// How many releases we request per page (the maximum GitHub allows).
const RELEASES_PER_PAGE: usize = 100;
/// Cap on the number of pages — so we do not hit the API request limit.
const MAX_RELEASE_PAGES: usize = 5;

/// List of recent sing-box releases. We filter out prereleases: updating to a
/// beta without an explicit request is not what the user expects.
///
/// We walk pages until we collect `limit` stable releases: between stable
/// versions there can be dozens of alpha and beta, and a single page may
/// simply not have enough of them.
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
                "GitHub responded {} when requesting the release list",
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
// Release catalog cache
//
// The release list is needed often (update checks, version selection) but
// changes once every few weeks. So in the UI it always comes from cache, and
// a trip to GitHub is either a background refresh at startup or an explicit button.
// ---------------------------------------------------------------------------

const CATALOG_FILE: &str = "releases.json";
/// How long the cache is considered fresh. Longer — refresh in the background at startup.
const CATALOG_TTL_SECS: u64 = 12 * 60 * 60;
/// How many stable releases we keep in the catalog.
pub const CATALOG_SIZE: usize = 15;

/// The release catalog as the UI sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCatalog {
    /// When the list was fetched from GitHub, unix time in seconds. 0 — never.
    pub fetched_at: u64,
    /// The cache needs a refresh.
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

/// Catalog from cache. Does not touch the network.
pub fn cached_catalog(active_version: Option<&str>) -> ReleaseCatalog {
    finish(read_cache(), active_version)
}

/// Refreshes the cache from GitHub and returns the fresh catalog.
pub async fn refresh_catalog(active_version: Option<&str>) -> Result<ReleaseCatalog> {
    let releases = fetch_releases(CATALOG_SIZE).await?;
    let cache = CachedCatalog {
        fetched_at: now_secs(),
        releases,
    };
    write_cache(&cache)?;
    Ok(finish(cache, active_version))
}

/// Whether the cache should be refreshed in the background.
pub fn catalog_is_stale() -> bool {
    let cache = read_cache();
    cache.releases.is_empty() || now_secs().saturating_sub(cache.fetched_at) > CATALOG_TTL_SECS
}

/// Completes the catalog with local facts: what is downloaded and what is active.
///
/// Downloaded versions that are not in the GitHub list (for example, a release
/// that fell out of the first `CATALOG_SIZE`) still make it into the catalog —
/// otherwise the installed version could not be deleted.
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

/// Asset name for the current OS and architecture.
///
/// For now we only support Windows: there assets are zips, and our unpacker is
/// the zip one. Linux and macOS ship tar.gz; their support arrives with M3.
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

/// Downloads an asset, computing sha256 on the fly.
///
/// Upstream does not publish checksums, so there is nothing to check the hash
/// against: integrity is provided by TLS to GitHub. The computed value is
/// shown to the user — it can be verified by hand.
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
            "failed to download {url}: {}",
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

/// Deletes a downloaded version. A missing file is not an error.
pub fn remove_version(version: &str) -> Result<()> {
    let path = version_path(version)?;
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Extracts the sing-box binary from a downloaded archive. Inside it lives in
/// a folder like `sing-box-1.13.16-windows-amd64/`, so we look it up by file name.
pub fn extract(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| Error::io(archive_path.display().to_string(), e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::Other(format!("could not read archive: {e}")))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| Error::Other(format!("could not read archive entry: {e}")))?;

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
        "no {EXE_NAME} file in the archive"
    )))
}

/// On Windows a child process would otherwise flash a console window.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}
