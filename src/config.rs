use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

use tracing::warn;

/// Ищет файл .env: сначала в текущей рабочей директории,
/// затем рядом с исполняемым файлом и выше по дереву (для target/release).
fn find_dotenv() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join(".env"));
    }
    candidates.push(PathBuf::from(".env"));

    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..5 {
            match dir {
                Some(d) => {
                    candidates.push(d.join(".env"));
                    dir = d.parent().map(|p| p.to_path_buf());
                }
                None => break,
            }
        }
    }

    candidates.into_iter().find(|p| p.is_file())
}

/// Приводит значение ProxyServer к URL прокси.
fn normalize_proxy(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.contains('=') {
        let mut https = None;
        let mut http = None;
        let mut socks = None;
        for part in raw.split(';') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                match k.trim().to_lowercase().as_str() {
                    "https" => https = Some(v.trim().to_string()),
                    "http" => http = Some(v.trim().to_string()),
                    "socks" => socks = Some(v.trim().to_string()),
                    _ => {}
                }
            }
        }
        if let Some(addr) = https.or(http) {
            return with_scheme(&addr, "http");
        }
        return socks.and_then(|s| with_scheme(&s, "socks5"));
    }
    with_scheme(raw, "http")
}

fn with_scheme(addr: &str, scheme: &str) -> Option<String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return None;
    }
    if addr.contains("://") {
        return Some(addr.to_string());
    }
    Some(format!("{}://{}", scheme, addr))
}

/// Прокси из системных настроек Windows (Internet Settings).
fn windows_system_proxy() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let key = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    let enabled = std::process::Command::new("reg")
        .args(["query", key, "/v", "ProxyEnable"])
        .output()
        .ok()?;
    if !String::from_utf8_lossy(&enabled.stdout).contains("0x1") {
        return None;
    }

    let server = std::process::Command::new("reg")
        .args(["query", key, "/v", "ProxyServer"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&server.stdout).to_string();
    let raw = text.lines().find_map(|l| l.split_whitespace().nth(2))?;
    normalize_proxy(raw)
}

/// Проверяет, что прокси действительно отвечает: иначе работаем напрямую.
fn proxy_reachable(url: &str) -> bool {
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    let rest = url.split("://").last().unwrap_or(url);
    let rest = rest.split('/').next().unwrap_or(rest);
    let default_port = if url.starts_with("socks") { 1080 } else { 8080 };
    let addr = match rest.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => format!("{}:{}", h, port),
            Err(_) => format!("{}:{}", rest, default_port),
        },
        None => format!("{}:{}", rest, default_port),
    };

    let Ok(mut addrs) = addr.to_socket_addrs() else {
        return false;
    };
    let Some(sa) = addrs.next() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&sa, Duration::from_millis(1500)).is_ok()
}

/// Собственный простой парсер .env (KEY=VALUE, поддержка комментариев
/// и кавычек). Не зависит от внешнего крейта.
fn parse_env_file(path: &PathBuf) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Не удалось прочитать .env ({}): {}", path.display(), e);
            return map;
        }
    };

    let content = content.trim_start_matches('\u{feff}');

    // Нормализуем переводы строк: CRLF → LF, затем оставшиеся CR → LF.
    // Это позволяет корректно читать .env с любым стилем переноса строк.
    let content = content.replace("\r\n", "\n").replace('\r', "\n");

    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            tracing::debug!("  строка {}: нет '=' -> '{}'", i + 1, line);
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            tracing::debug!("  строка {}: пустой ключ -> '{}'", i + 1, line);
            continue;
        }
        let mut value = value.trim().to_string();

        if value.len() >= 2 {
            let first = value.as_bytes()[0];
            let last = value.as_bytes()[value.len() - 1];
            if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
                value = value[1..value.len() - 1].to_string();
            }
        }

        map.insert(key.to_string(), value.clone());
        tracing::debug!("  строка {}: ключ='{}', значение='{}'", i + 1, key, value);
    }

    map
}

/// Переменные из .env, поверх — реальные переменные окружения (приоритет).
/// Пустые переменные окружения НЕ перезаписывают значения из .env.
fn load_vars() -> HashMap<String, String> {
    let mut vars = HashMap::new();

    if let Some(path) = find_dotenv() {
        tracing::info!("Загружен .env: {}", path.display());
        let parsed = parse_env_file(&path);
        tracing::info!("Из .env загружены ключи: {:?}", parsed.keys().collect::<Vec<_>>());
        vars.extend(parsed);
    } else {
        warn!(".env не найден — используются только переменные окружения");
    }

    // Переменные окружения имеют приоритет, но только если не пустые
    for (k, v) in env::vars() {
        let v = v.trim().to_string();
        if !v.is_empty() {
            vars.insert(k, v);
        } else if vars.contains_key(&k) {
            warn!("Переменная окружения {} пуста, игнорирую (остаётся значение из .env)", k);
        }
    }

    vars
}

/// Конфигурация бота, загружаемая из .env файла.
pub struct Config {
    pub bot_token: String,
    pub api_id: i64,
    pub api_hash: String,
    pub session_name: String,
    pub userbot_2fa_password: String,
    pub database_path: PathBuf,
    pub owner_username: String,
    pub developer_username: String,
    pub bot_username: String,
    pub rules_url: String,
    pub allowed_user_ids: HashSet<i64>,
    pub session_cooldown_seconds: i64,
    pub pool_size: i32,
    pub pool_refill_interval_seconds: i64,
    pub proxy_url: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let vars = load_vars();
        let get = |key: &str| -> Option<&str> {
            vars.get(key)
                .map(|s| s.as_str())
                .filter(|s| !s.trim().is_empty())
        };

        let bot_token = match get("BOT_TOKEN") {
            Some(t) => t.trim().to_string(),
            None => {
                let mut keys: Vec<String> = vars.keys().cloned().collect();
                keys.sort();
                panic!(
                    "BOT_TOKEN не задан. Добавьте строку BOT_TOKEN=... в файл .env рядом с ботом.\n\
                     Найденные ключи окружения: {:?}",
                    keys
                )
            }
        };

        let api_id = get("API_ID")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let api_hash = get("API_HASH").unwrap_or("").to_string();

        let database_path = Self::resolve_db_path(get("DATABASE_URL"));

        let allowed_user_ids: HashSet<i64> = get("ALLOWED_USER_IDS")
            .unwrap_or("")
            .split([' ', ',', ';'])
            .filter_map(|s| {
                let s = s.trim();
                if s.is_empty() {
                    None
                } else {
                    s.parse::<i64>().ok()
                }
            })
            .collect();

        if allowed_user_ids.is_empty() {
            warn!("ALLOWED_USER_IDS не задан — бот будет отказывать в доступе ВСЕМ!");
        }

        let session_cooldown_seconds = get("SESSION_COOLDOWN_SECONDS")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(120)
            .max(30);

        let pool_size = get("POOL_SIZE")
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(50)
            .clamp(1, 70);

        let pool_refill_interval_seconds = get("POOL_REFILL_INTERVAL_SECONDS")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(180)
            .max(30);

        let proxy_url = get("PROXY_URL")
            .or_else(|| get("TELOXIDE_PROXY"))
            .or_else(|| get("HTTPS_PROXY"))
            .or_else(|| get("https_proxy"))
            .or_else(|| get("HTTP_PROXY"))
            .map(|s| s.trim().to_string())
            .or_else(windows_system_proxy)
            .filter(|p| {
                if proxy_reachable(p) {
                    true
                } else {
                    warn!("Прокси {} недоступен — работаем напрямую", p);
                    false
                }
            });

        Self {
            bot_token,
            api_id,
            api_hash,
            session_name: get("SESSION_NAME").unwrap_or("alyaska_userbot").to_string(),
            userbot_2fa_password: get("USERBOT_2FA_PASSWORD").unwrap_or("").to_string(),
            database_path,
            owner_username: get("OWNER_USERNAME").unwrap_or("Valigonov").to_string(),
            developer_username: get("DEVELOPER_USERNAME")
                .unwrap_or("NyawkaCuteUwU")
                .to_string(),
            bot_username: get("BOT_USERNAME")
                .unwrap_or("AlyaskaGarantBot")
                .to_string(),
            rules_url: get("RULES_URL")
                .unwrap_or("https://t.me/ValigonovRules/17")
                .to_string(),
            allowed_user_ids,
            session_cooldown_seconds,
            pool_size,
            pool_refill_interval_seconds,
            proxy_url,
        }
    }

    /// Определяет путь к файлу БД.
    fn resolve_db_path(database_url: Option<&str>) -> PathBuf {
        if let Some(url) = database_url {
            let url = url.trim();
            if url.starts_with("sqlite:") {
                let cleaned = url
                    .trim_start_matches("sqlite://")
                    .trim_start_matches("sqlite:");
                if !cleaned.is_empty() && cleaned != "data.db" {
                    return PathBuf::from(cleaned);
                }
            }
        }

        let base = find_dotenv()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        base.join("data.db")
    }

    pub fn is_allowed(&self, user_id: i64) -> bool {
        self.allowed_user_ids.contains(&user_id)
    }

    pub fn is_owner_or_dev(&self, username: Option<&str>) -> bool {
        if let Some(username) = username {
            let clean = username.trim_start_matches('@').to_lowercase();
            clean == self.owner_username.to_lowercase()
                || clean == self.developer_username.to_lowercase()
        } else {
            false
        }
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn config() -> &'static Config {
    CONFIG.get_or_init(Config::load)
}
