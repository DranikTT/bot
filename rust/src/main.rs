//! Точка входа: загрузка конфига, long-polling и вызов обработчиков.

use savemod_bot::api::{log, Api};
use savemod_bot::config::Config;
use savemod_bot::handlers::Handlers;
use savemod_bot::sender::Sender;
use savemod_bot::store::Store;

fn main() {
    if let Err(error) = run() {
        eprintln!("✖ {}", error);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = Config::load()?;
    log::set_verbose(config.verbose);

    log::info(format!(
        "SaveMod на Rust — токен {}, админов: {}, база: {}",
        mask_token(&config.bot_token),
        config.admin_ids.len(),
        config.database_path.display()
    ));

    let api = Api::new(&config.bot_token, &config.api_base);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let sender = Sender::new(&config);

    // проверка баннеров при старте: лучше узнать сразу, чем на первом клике
    for key in ["main", "settings", "subscription", "admin"] {
        if sender.banner_is_valid(key) {
            log::info(format!("баннер «{}» найден", key));
        } else {
            log::warn(format!(
                "баннер «{}» отсутствует или не картинка — меню уйдёт текстом",
                key
            ));
        }
    }

    let me = api.get_me().map_err(|error| error.to_string())?;
    let bot_username = me.get_str("username");
    let bot_id = me.get_i64("id", 0);
    log::info(format!(
        "подключён как @{} (id {})",
        if bot_username.is_empty() { "?" } else { &bot_username },
        bot_id
    ));

    // вебхук не нужен: работаем через long polling
    api.delete_webhook().map_err(|error| error.to_string())?;

    let _ = api.set_my_commands(vec![
        ("start", "Главное меню"),
        ("help", "Справка"),
        ("archive", "Экспорт переписки"),
        ("import", "Импорт из Telegram Desktop"),
    ]);

    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, &bot_username);
    let mut offset: i64 = 0;
    let mut failures: u32 = 0;

    log::info("ожидаю обновления");

    loop {
        match api.get_updates(offset, 30) {
            Ok(updates) => {
                failures = 0;
                for update in updates {
                    let update_id = update.get_i64("update_id", 0);
                    if update_id > 0 {
                        offset = update_id + 1;
                    }

                    if config.verbose {
                        log::debug(format!("update {}", update_id));
                    }

                    // паника в обработчике не должна ронять бота
                    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        handlers.handle(&update);
                    }));
                    if handled.is_err() {
                        log::error(format!("исключение при обработке update {}", update_id));
                    }
                    handlers.flush();
                }
            }
            Err(error) => {
                failures += 1;
                log::error(format!("getUpdates: {}", error));

                // rate limit — ждём указанное время
                let wait = if error.is_rate_limit() {
                    error.retry_after_seconds().max(1).min(60)
                } else {
                    // экспоненциальная задержка: 1, 2, 4, ... до 30 секунд
                    (1u64 << failures.min(5)).min(30)
                };
                std::thread::sleep(std::time::Duration::from_secs(wait));
            }
        }
    }
}

/// Не светим токен в логах.
fn mask_token(token: &str) -> String {
    match token.split_once(':') {
        Some((id, secret)) if secret.len() > 6 => {
            format!("{}:{}…{}", id, &secret[..3], &secret[secret.len() - 3..])
        }
        _ => "***".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn токен_маскируется() {
        let masked = mask_token("123456:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw");
        assert!(masked.starts_with("123456:"));
        assert!(masked.contains('…'));
        assert!(!masked.contains("TcvCH1vGWJxfSeofSAs0K5PALDsaw"));
    }

    #[test]
    fn короткий_токен_полностью_скрыт() {
        assert_eq!(mask_token("abc"), "***");
    }
}
