//! Подготовка рантайм-копии конфига sing-box.
//!
//! Пользовательский `config.json` мы никогда не переписываем. Перед запуском
//! сервиса рядом с настройками появляется `runtime.json` — та же конфигурация,
//! но с гарантированно включённым Clash API и подставленным secret'ом.
//! Secret не живёт в `settings.json`: он генерируется на лету и хранится
//! в отдельном `runtime-state.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::jsonc::strip_jsonc;
use crate::settings::{config_dir, ClashApiSettings, Settings};

const RUNTIME_CONFIG: &str = "runtime.json";
const RUNTIME_STATE: &str = "runtime-state.json";

/// Что получилось после подготовки — этим запускается sing-box.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedConfig {
    /// Путь к рантайм-копии, её отдаём sing-box через `-c`.
    pub config_path: String,
    /// `host:port`, на котором будет слушать Clash API.
    pub external_controller: String,
    /// Пришёл ли secret из пользовательского конфига (тогда мы его не трогали).
    pub secret_from_user_config: bool,
    /// Действующий secret. Наружу не отдаётся: структура уходит во фронтенд,
    /// а показывать там токен управления незачем.
    #[serde(skip)]
    pub secret: String,
}

/// То, что нужно знать GUI после старта сервиса, но чего нет в настройках.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuntimeState {
    pub api_url: String,
    pub secret: String,
}

pub fn runtime_config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(RUNTIME_CONFIG))
}

fn runtime_state_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(RUNTIME_STATE))
}

/// Собирает рантайм-конфиг из пользовательского и записывает его на диск.
pub fn prepare(settings: &Settings) -> Result<PreparedConfig> {
    prepare_in(&config_dir()?, settings)
}

/// То же, но с явной директорией для рантайм-файлов.
///
/// Отдельный параметр нужен инструментам, которые прогоняют несколько версий
/// sing-box подряд: каждой нужна своя песочница, и подменять переменную
/// окружения на ходу ради этого не годится.
pub fn prepare_in(dir: &Path, settings: &Settings) -> Result<PreparedConfig> {
    let source = settings.sing_box.config_path.trim();
    if source.is_empty() {
        return Err(Error::Other(
            "путь к config.json sing-box не задан — укажите его в настройках".into(),
        ));
    }

    let raw = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let mut config: Value = serde_json::from_str(&strip_jsonc(&raw))
        .map_err(|e| Error::parse(source, e))?;

    if !config.is_object() {
        return Err(Error::Other(format!(
            "{source}: ожидался JSON-объект в корне конфига"
        )));
    }

    let external_controller = host_port(&settings.clash_api.url);

    // Уже заданный пользователем secret — его решение, уважаем и переиспользуем.
    let existing_secret = config
        .pointer("/experimental/clash_api/secret")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let secret_from_user_config = existing_secret.is_some();
    let secret = match existing_secret {
        Some(secret) => secret,
        None if !settings.clash_api.secret.is_empty() => settings.clash_api.secret.clone(),
        None => generate_secret(),
    };

    // Без Clash API приложение слепое, поэтому блок доводим до рабочего вида
    // независимо от того, что было в исходном конфиге. Остальные ключи
    // clash_api (external_ui, default_mode и прочее) сохраняем как есть.
    let root = config.as_object_mut().expect("проверено выше");
    let experimental = root.entry("experimental").or_insert_with(|| json!({}));
    if !experimental.is_object() {
        *experimental = json!({});
    }
    let clash_api = experimental
        .as_object_mut()
        .expect("только что привели к объекту")
        .entry("clash_api")
        .or_insert_with(|| json!({}));
    if !clash_api.is_object() {
        *clash_api = json!({});
    }
    clash_api["external_controller"] = json!(external_controller);
    clash_api["secret"] = json!(secret.clone());

    let path = dir.join(RUNTIME_CONFIG);
    write_private(&path, &serde_json::to_vec_pretty(&config).unwrap_or_default())?;

    write_private(
        &dir.join(RUNTIME_STATE),
        &serde_json::to_vec_pretty(&RuntimeState {
            api_url: settings.clash_api.url.clone(),
            secret: secret.clone(),
        })
        .unwrap_or_default(),
    )?;

    Ok(PreparedConfig {
        config_path: path.display().to_string(),
        external_controller,
        secret_from_user_config,
        secret,
    })
}

/// Нужен ли конфигу TUN-инбаунд.
///
/// От этого зависит, обязателен ли сервис: TUN поднимает сетевой адаптер и
/// требует прав администратора, всё остальное запускается обычным процессом.
pub fn requires_tun(settings: &Settings) -> Result<bool> {
    let source = settings.sing_box.config_path.trim();
    if source.is_empty() {
        return Err(Error::Other(
            "путь к config.json sing-box не задан — укажите его в настройках".into(),
        ));
    }

    let raw = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let config: Value =
        serde_json::from_str(&strip_jsonc(&raw)).map_err(|e| Error::parse(source, e))?;

    Ok(config
        .get("inbounds")
        .and_then(Value::as_array)
        .is_some_and(|inbounds| {
            inbounds
                .iter()
                .any(|inbound| inbound.get("type").and_then(Value::as_str) == Some("tun"))
        }))
}

pub fn load_state() -> RuntimeState {
    let Ok(path) = runtime_state_path() else {
        return RuntimeState::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Настройки подключения с уже подставленным действующим secret'ом.
///
/// Приоритет у явного значения из `settings.json`: если пользователь его
/// прописал, он ожидает именно его. Иначе берём то, с чем мы сами запускали
/// sing-box — но только если адрес API с тех пор не менялся.
pub fn effective_api_settings(settings: &Settings) -> ClashApiSettings {
    if !settings.clash_api.secret.is_empty() {
        return settings.clash_api.clone();
    }

    let state = load_state();
    if state.secret.is_empty() || state.api_url != settings.clash_api.url {
        return settings.clash_api.clone();
    }

    ClashApiSettings {
        secret: state.secret,
        ..settings.clash_api.clone()
    }
}

/// Файлы с secret'ом кладём доступными только владельцу. На Windows это уже
/// обеспечено ACL пользовательского профиля, на Unix нужно выставить режим явно.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    std::fs::write(path, body).map_err(|e| Error::io(path.display().to_string(), e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// 16 случайных байт в hex. Берём системный CSPRNG — secret защищает
/// локальный порт управления, слабая энтропия тут не годится.
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Единственный сценарий отказа — недоступный системный источник
        // энтропии. Тогда честнее не подставлять предсказуемое значение,
        // а оставить API без авторизации: он всё равно только на loopback.
        return String::new();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `http://127.0.0.1:9090/` → `127.0.0.1:9090`.
fn host_port(url: &str) -> String {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let host = without_scheme
        .split(['/', '?'])
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('/');
    if host.is_empty() {
        "127.0.0.1:9090".into()
    } else {
        host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_host_port() {
        assert_eq!(host_port("http://127.0.0.1:9090"), "127.0.0.1:9090");
        assert_eq!(host_port("http://127.0.0.1:9090/"), "127.0.0.1:9090");
        assert_eq!(host_port("127.0.0.1:9090"), "127.0.0.1:9090");
        assert_eq!(host_port(""), "127.0.0.1:9090");
    }

    /// Песочница на один тест: конфиг пользователя и директория рантайма.
    fn sandbox(name: &str, config: &str) -> (PathBuf, Settings) {
        let dir = std::env::temp_dir().join(format!("vantage-box-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("config.json");
        std::fs::write(&config_path, config).unwrap();

        let mut settings = Settings::default();
        settings.sing_box.config_path = config_path.display().to_string();
        (dir, settings)
    }

    #[test]
    fn detects_tun_inbound() {
        let (_, with_tun) = sandbox(
            "tun",
            r#"{"inbounds":[{"type":"tun","tag":"tun-in"}],"outbounds":[]}"#,
        );
        assert!(requires_tun(&with_tun).unwrap());

        let (_, without_tun) = sandbox(
            "no-tun",
            r#"{"inbounds":[{"type":"mixed","listen_port":2080}]}"#,
        );
        assert!(!requires_tun(&without_tun).unwrap());

        let (_, empty) = sandbox("no-inbounds", r#"{"outbounds":[]}"#);
        assert!(!requires_tun(&empty).unwrap());
    }

    /// Пользовательский конфиг должен остаться узнаваемым: порядок ключей тот
    /// же, а наш блок дописан в конец — тогда номера строк в ошибках sing-box
    /// совпадают с исходным файлом настолько, насколько это вообще возможно.
    #[test]
    fn keeps_key_order_and_appends_our_block() {
        let (dir, settings) = sandbox(
            "order",
            r#"{"log":{"level":"info"},"dns":{},"inbounds":[],"outbounds":[]}"#,
        );

        prepare_in(&dir, &settings).unwrap();
        let written = std::fs::read_to_string(dir.join(RUNTIME_CONFIG)).unwrap();

        let keys: Vec<&str> = ["log", "dns", "inbounds", "outbounds", "experimental"]
            .into_iter()
            .filter(|key| written.contains(&format!("\"{key}\"")))
            .collect();
        assert_eq!(keys, ["log", "dns", "inbounds", "outbounds", "experimental"]);
        assert!(written.find("\"log\"").unwrap() < written.find("\"experimental\"").unwrap());
    }

    /// Уже заданный external_controller правим на месте, а не добавляем второй.
    #[test]
    fn replaces_existing_field_in_place() {
        let (dir, settings) = sandbox(
            "in-place",
            r#"{"experimental":{"clash_api":{"external_controller":"127.0.0.1:1","external_ui":"ui"}},"log":{}}"#,
        );

        let prepared = prepare_in(&dir, &settings).unwrap();
        let written = std::fs::read_to_string(dir.join(RUNTIME_CONFIG)).unwrap();

        assert!(written.contains(&prepared.external_controller));
        assert!(!written.contains("127.0.0.1:1\""));
        // Соседние ключи не трогаем: это настройки пользователя.
        assert!(written.contains("external_ui"));
        // Блок остался первым — переносить его вниз незачем.
        assert!(written.find("\"experimental\"").unwrap() < written.find("\"log\"").unwrap());
    }

    #[test]
    fn generates_distinct_secrets() {
        let a = generate_secret();
        let b = generate_secret();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
