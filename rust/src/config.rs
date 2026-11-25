//! Конфигурация: `.env` и переменные окружения.
//!
//! Секретов в коде нет: токен и ключ читаются из файла/окружения.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Config {
    pub bot_token: String,
    pub admin_ids: Vec<i64>,
    pub database_path: PathBuf,
    pub encryption_key: String,
    pub api_base: String,
    pub assets_dir: PathBuf,
    pub max_export_mb: i64,
    pub max_single_media_mb: i64,
    pub max_import_file_mb: i64,
    pub max_imported_media_mb: i64,
    pub custom_emoji: bool,
    pub verbose: bool,
    pub temp_dir: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let root = resolve_root();
        let env = read_env(&root.join(".env"));

        let bot_token = env_value(&env, "BOT_TOKEN");
        if bot_token.trim().is_empty() {
            return Err(format!(
                "Не задан BOT_TOKEN. Добавьте его в {} или в переменные окружения.",
                root.join(".env").display()
            ));
        }

        let admin_ids = parse_ids(&env_value(&env, "ADMIN_IDS"));
        let database_path = match env_value(&env, "DATABASE_PATH") {
            value if value.trim().is_empty() => root.join("bot_database.json"),
            value => PathBuf::from(value),
        };

        let temp_dir = root.join("temp");
        let _ = std::fs::create_dir_all(&temp_dir);

        let config = Config {
            bot_token,
            admin_ids,
            database_path,
            encryption_key: env_value(&env, "DB_ENCRYPTION_KEY"),
            api_base: match env_value(&env, "API_SERVER") {
                value if value.trim().is_empty() => "https://api.telegram.org".to_string(),
                value => value,
            },
            assets_dir: root.join("assets"),
            max_export_mb: parse_number(&env_value(&env, "MAX_EXPORT_MB"), 48),
            max_single_media_mb: parse_number(&env_value(&env, "MAX_SINGLE_MEDIA_MB"), 35),
            max_import_file_mb: parse_number(&env_value(&env, "MAX_IMPORT_FILE_MB"), 200),
            max_imported_media_mb: parse_number(&env_value(&env, "MAX_IMPORTED_MEDIA_MB"), 100),
            custom_emoji: !env_value(&env, "CUSTOM_EMOJI").eq_ignore_ascii_case("off"),
            verbose: env_value(&env, "VERBOSE").eq_ignore_ascii_case("on"),
            temp_dir,
        };

        if !config.custom_emoji {
            crate::emoji::disable_by_config();
        }

        Ok(config)
    }

    pub fn is_admin(&self, user_id: i64) -> bool {
        self.admin_ids.contains(&user_id)
    }

    /// Путь к баннеру по ключу меню.
    pub fn banner(&self, key: &str) -> PathBuf {
        let name = match key {
            "main" => "mainmenu.jpg",
            "settings" => "settings.jpg",
            "subscription" => "subscription.jpg",
            "admin" => "adminmenu.jpg",
            other => other,
        };
        self.assets_dir.join(name)
    }
}

/// Корень проекта: ищем каталог с assets/ или .env, поднимаясь вверх.
fn resolve_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let is_root = |path: &Path| {
        path.join("assets").is_dir() || path.join(".env").is_file()
    };

    if is_root(&cwd) {
        return cwd;
    }
    // запуск из rust/ или target/
    for ancestor in cwd.ancestors().skip(1).take(3) {
        if is_root(ancestor) {
            return ancestor.to_path_buf();
        }
    }
    cwd
}

fn read_env(path: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return values;
    };

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap();
        let mut value = value.trim().to_string();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = value[1..value.len() - 1].to_string();
        }
        let key = key.trim().to_string();
        if !key.is_empty() {
            values.insert(key, value);
        }
    }
    values
}

/// Переменная окружения важнее .env. Для ключа шифрования есть дефолт,
/// совпадающий с прежними версиями, чтобы старая база читалась.
fn env_value(env: &BTreeMap<String, String>, key: &str) -> String {
    if let Ok(value) = std::env::var(key) {
        if !value.trim().is_empty() {
            return value.trim().to_string();
        }
    }
    if let Some(value) = env.get(key) {
        return value.clone();
    }
    if key == "DB_ENCRYPTION_KEY" {
        return "SaveMod_AES256_Default_Secret_Key_2025_Secure!".to_string();
    }
    String::new()
}

fn parse_ids(raw: &str) -> Vec<i64> {
    raw.split([',', ' ', '\t', '\n'])
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
        .collect()
}

fn parse_number(raw: &str, fallback: i64) -> i64 {
    raw.trim().parse::<i64>().ok().filter(|value| *value > 0).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn разбирает_список_админов() {
        assert_eq!(parse_ids("1, 2,3"), vec![1, 2, 3]);
        assert_eq!(parse_ids(" 10  20 "), vec![10, 20]);
        assert_eq!(parse_ids("abc, -5, 0"), Vec::<i64>::new());
        assert_eq!(parse_ids(""), Vec::<i64>::new());
    }

    #[test]
    fn числа_с_дефолтом() {
        assert_eq!(parse_number("30", 10), 30);
        assert_eq!(parse_number("abc", 10), 10);
        assert_eq!(parse_number("-5", 10), 10);
        assert_eq!(parse_number("", 10), 10);
    }

    #[test]
    fn имена_баннеров_как_в_конфиге() {
        let config = Config {
            bot_token: "t".to_string(),
            admin_ids: vec![],
            database_path: PathBuf::from("db.json"),
            encryption_key: "k".to_string(),
            api_base: "https://api.telegram.org".to_string(),
            assets_dir: PathBuf::from("/app/assets"),
            max_export_mb: 48,
            max_single_media_mb: 35,
            max_import_file_mb: 200,
            max_imported_media_mb: 100,
            custom_emoji: true,
            verbose: false,
            temp_dir: PathBuf::from("/tmp"),
        };
        assert!(config.banner("main").ends_with("mainmenu.jpg"));
        assert!(config.banner("settings").ends_with("settings.jpg"));
        assert!(config.banner("subscription").ends_with("subscription.jpg"));
        assert!(config.banner("admin").ends_with("adminmenu.jpg"));
    }

    #[test]
    fn админ_определяется_по_id() {
        let config = Config {
            bot_token: "t".to_string(),
            admin_ids: vec![1, 2],
            database_path: PathBuf::from("db.json"),
            encryption_key: "k".to_string(),
            api_base: "https://api.telegram.org".to_string(),
            assets_dir: PathBuf::from("/app/assets"),
            max_export_mb: 48,
            max_single_media_mb: 35,
            max_import_file_mb: 200,
            max_imported_media_mb: 100,
            custom_emoji: true,
            verbose: false,
            temp_dir: PathBuf::from("/tmp"),
        };
        assert!(config.is_admin(1));
        assert!(!config.is_admin(3));
    }
}
