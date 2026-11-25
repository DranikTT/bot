//! Тексты меню с кастомными эмодзи через `<tg-emoji>`.

use crate::config::Config;
use crate::emoji::{escape, html};
use crate::json::Value;
use crate::store::{now, Store};

/// Готовое меню: текст, клавиатура, ключ баннера (может отсутствовать).
pub struct Menu {
    pub text: String,
    pub keyboard: Option<Value>,
    pub banner: Option<&'static str>,
}

impl Menu {
    pub fn new(text: String, keyboard: Option<Value>) -> Self {
        Menu {
            text,
            keyboard,
            banner: None,
        }
    }

    pub fn with_banner(text: String, keyboard: Option<Value>, banner: &'static str) -> Self {
        Menu {
            text,
            keyboard,
            banner: Some(banner),
        }
    }
}

/// Дата из секунд в формате дд.мм.гггг чч:мм.
pub fn format_time(seconds: i64) -> String {
    if seconds <= 0 {
        return String::new();
    }
    let days = seconds / 86400;
    let seconds_of_day = seconds % 86400;

    let (hours, minutes) = (
        (seconds_of_day / 3600) % 24,
        (seconds_of_day % 3600) / 60,
    );

    let mut year = 1970;
    let mut remaining = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let year_days = if leap { 366 } else { 365 };
        if remaining < year_days {
            break;
        }
        remaining -= year_days;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for days_in_month in month_days {
        if remaining < days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }

    format!(
        "{:02}.{:02}.{:04} {:02}:{:02}",
        remaining + 1,
        month,
        year,
        hours,
        minutes
    )
}

/// Ник пользователя.
pub fn nick(user: &Value) -> String {
    let first_name = user.get_str("first_name");
    let name = if first_name.trim().is_empty() {
        "Пользователь".to_string()
    } else {
        escape(first_name.trim())
    };
    let username = user.get_str("username");
    if username.trim().is_empty() {
        name
    } else {
        format!("{} (@{})", name, escape(username.trim()))
    }
}

/// Статус подписки строкой.
pub fn subscription_status(user: &Value, config: &Config) -> String {
    let user_id = user.get_i64("user_id", 0);

    if user.get_bool("is_banned", false) {
        return "заблокирована (бан)".to_string();
    }
    if config.is_admin(user_id) {
        return "безлимит (админ)".to_string();
    }

    let until = user.get_i64("subscription_until", 0);
    let current = now();

    if until > current {
        let days = (until - current + 86399) / 86400;
        let premium = if user.get_bool("is_premium", false) {
            "Премиум "
        } else {
            ""
        };
        format!(
            "{}активна до {} ({} дн.)",
            premium,
            format_time(until),
            days
        )
    } else {
        "не активна".to_string()
    }
}

// ------------------------------------------------------------- меню

pub fn main(user: &Value, store: &Store, config: &Config) -> Menu {
    let user_id = user.get_i64("user_id", 0);
    let connected = store.connection_of(user_id).is_some();

    let text = format!(
        "{} Привет, <b>{}</b>!\n\n{} Статус подписки: <b>{}</b>\n{} Подключение: <b>{}</b>\n\n{} Выберите раздел:",
        html("wave", "👋"),
        nick(user),
        html("shield", "🛡"),
        subscription_status(user, config),
        html("clip", "📎"),
        if connected { "подключён" } else { "не подключён" },
        html("arrow", "🔽"),
    );

    Menu::with_banner(
        text,
        Some(crate::keyboard::main(user_id, config.is_admin(user_id))),
        "main",
    )
}

pub fn settings(user: &Value) -> Menu {
    let deleted = user.get_bool("notify_deleted", true);
    let edited = user.get_bool("notify_edited", true);

    let text = format!(
        "{} <b>Настройки</b>\n\n\
         • Уведомления об удалённых: <b>{}</b>\n\
         • Уведомления об изменённых: <b>{}</b>\n\
         • Перехват сгорающих медиа (точка <code>.</code> или <code>,</code>): <b>активен</b>",
        html("gear", "⚙️"),
        if deleted { "включены" } else { "выключены" },
        if edited { "включены" } else { "выключены" },
    );

    Menu::with_banner(
        text,
        Some(crate::keyboard::settings(deleted, edited)),
        "settings",
    )
}

pub fn connect(bot_username: &str) -> Menu {
    let text = format!(
        "{} <b>Подключение к Telegram Business</b>\n\n\
         Подключение выполняет владелец аккаунта вручную — бот не может \
         сделать это сам.\n\n\
         1. Откройте <b>Профиль</b> → <b>Изменить</b>\n\
         2. Перейдите в <b>Автоматизация чатов</b>\n\
         3. Укажите юзернейм <code>@{}</code>\n\
         4. Нажмите <b>Добавить</b> и выдайте права на чтение и ответы\n\n\
         После этого нажмите «Проверить подключение».",
        html("plug", "🔌"),
        escape(bot_username),
    );
    Menu::new(text, Some(crate::keyboard::connect()))
}

pub fn tariffs(user: &Value, config: &Config) -> Menu {
    let text = format!(
        "{} <b>Тарифы</b>\n\n\
         Ваш статус: <b>{}</b>\n\n\
         Оплата проходит звёздами Telegram. Подписка продлевается, \
         а не заменяет текущую.",
        html("gem", "💎"),
        subscription_status(user, config),
    );
    Menu::with_banner(text, Some(crate::keyboard::tariffs()), "subscription")
}

pub fn premium(user: &Value, store: &Store, _config: &Config) -> Menu {
    let is_premium = store.has_premium(user.get_i64("user_id", 0), false);

    let text = format!(
        "{} <b>Премиум — максимальный контроль</b>\n\n\
         Статус: <b>{}</b>\n\n\
         Премиум оформляется на 365 дней.\n\n\
         Команды <code>.nah</code>, <code>.love</code>, <code>.bypass</code>, \
         <code>.mute</code> и <code>.unmute</code> работают и с обычной подпиской.",
        html("star", "⭐️"),
        if is_premium { "активен ✅" } else { "не подключён ❌" },
    );
    Menu::with_banner(text, Some(crate::keyboard::premium()), "subscription")
}

pub fn invite(user: &Value, bot_username: &str) -> Menu {
    let user_id = user.get_i64("user_id", 0);
    let link = format!("https://t.me/{}?start=ref_{}", bot_username, user_id);

    let text = format!(
        "{} <b>Пригласить друга</b>\n\n\
         • Вам за каждого друга: <b>+7 дней подписки</b>\n\
         • Другу при регистрации: <b>+3 дня подписки</b>\n\n\
         Приглашено: <b>{}</b>\n\n\
         Ваша ссылка:\n<code>{}</code>",
        html("folder", "👥"),
        user.get_i64("referrals_count", 0),
        escape(&link),
    );
    Menu::new(text, Some(crate::keyboard::invite()))
}

pub fn admin(store: &Store) -> Menu {
    let (total, active, premium, banned, connections) = store.stats();

    let text = format!(
        "{} <b>Админ-панель</b>\n\n\
         {} Пользователей: <b>{}</b>\n\
         {} Активных подписок: <b>{}</b>\n\
         {} Премиум: <b>{}</b>\n\
         {} Забанено: <b>{}</b>\n\
         {} Подключений: <b>{}</b>\n\n\
         Выберите действие:",
        html("crown", "👑"),
        html("stats", "📊"),
        total,
        html("gem", "💎"),
        active,
        html("star", "⭐️"),
        premium,
        html("cross", "🚫"),
        banned,
        html("plug", "🔌"),
        connections,
    );
    Menu::with_banner(text, Some(crate::keyboard::admin()), "admin")
}

pub fn archive_types(chat_id: i64, title: &str, selected: &[String]) -> Menu {
    let text = format!(
        "{} <b>Настройка экспорта</b>\n\
         {} Собеседник: <b>{}</b> (<code>{}</code>)\n\n\
         <b>Выберите типы сообщений:</b>",
        html("folder", "📁"),
        html("user", "👤"),
        escape(title),
        chat_id,
    );
    Menu::new(text, Some(crate::keyboard::archive_types(selected)))
}

pub fn chats_list(chats: &[(i64, String)]) -> Menu {
    let text = format!(
        "{} <b>Выберите чат для экспорта</b>\n\nНайдено диалогов: <b>{}</b>",
        html("folder", "📁"),
        chats.len(),
    );
    Menu::new(text, Some(crate::keyboard::chats(chats)))
}

pub fn empty_chats() -> Menu {
    let text = format!(
        "{} <b>Список чатов пуст</b>\n\n\
         В базе пока нет переписок. Подключите бота в Telegram Business \
         или импортируйте историю через /archive.",
        html("folder", "📁"),
    );
    Menu::new(text, Some(crate::keyboard::empty_chats()))
}

pub fn import_wait() -> Menu {
    let text = format!(
        "{} <b>Импорт истории Telegram Desktop</b>\n\n\
         Отправьте файл <code>result.json</code> или ZIP-архив экспорта.",
        html("clip", "📥"),
    );
    Menu::new(text, Some(crate::keyboard::import_wait()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emoji;

    #[test]
    fn форматирует_дату() {
        // 2023-11-14 22:13:20 UTC
        assert_eq!(format_time(1700000000), "14.11.2023 22:13");
        assert_eq!(format_time(0), "");
    }

    #[test]
    fn ник_с_юзернеймом() {
        let mut user = Value::obj();
        user.set("first_name", "Вася");
        user.set("username", "vasya");
        assert_eq!(nick(&user), "Вася (@vasya)");
    }

    #[test]
    fn ник_без_юзернейма() {
        let mut user = Value::obj();
        user.set("first_name", "Вася");
        assert_eq!(nick(&user), "Вася");
    }

    #[test]
    fn ник_экранирует_html() {
        let mut user = Value::obj();
        user.set("first_name", "<script>");
        assert!(nick(&user).contains("&lt;script&gt;"));
    }

    #[test]
    fn меню_содержит_кастомные_эмодзи() {
        emoji::reset_for_tests();
        let config = test_config();
        let store = test_store();
        let mut user = Value::obj();
        user.set("user_id", 1i64);
        user.set("first_name", "Тест");

        let menu = main(&user, &store, &config);
        assert!(menu.text.contains("<tg-emoji emoji-id=\""));
        assert!(menu.text.contains("</tg-emoji>"));
        assert_eq!(menu.banner, Some("main"));
    }

    #[test]
    fn меню_без_права_на_эмодзи_чистое() {
        emoji::reset_for_tests();
        emoji::mark_not_allowed();
        let config = test_config();
        let store = test_store();
        let mut user = Value::obj();
        user.set("user_id", 1i64);
        user.set("first_name", "Тест");

        let menu = main(&user, &store, &config);
        assert!(!menu.text.contains("<tg-emoji"), "теги остались: {}", menu.text);
        assert!(menu.text.contains("Привет"));
        emoji::reset_for_tests();
    }

    #[test]
    fn статус_подписки_админа() {
        let config = test_config();
        let mut user = Value::obj();
        user.set("user_id", 1i64);
        assert_eq!(subscription_status(&user, &config), "безлимит (админ)");
    }

    #[test]
    fn статус_подписки_обычного() {
        let config = Config {
            bot_token: "t".into(),
            admin_ids: vec![],
            database_path: std::path::PathBuf::from("db.json"),
            encryption_key: "k".into(),
            api_base: "https://api.telegram.org".into(),
            assets_dir: std::path::PathBuf::from("/tmp"),
            max_export_mb: 48,
            max_single_media_mb: 35,
            max_import_file_mb: 200,
            max_imported_media_mb: 100,
            custom_emoji: true,
            verbose: false,
            temp_dir: std::path::PathBuf::from("/tmp"),
        };
        let mut user = Value::obj();
        user.set("user_id", 5i64);
        assert_eq!(subscription_status(&user, &config), "не активна");

        user.set("subscription_until", now() + 10 * 86400);
        let status = subscription_status(&user, &config);
        assert!(status.contains("активна до"), "статус: {}", status);
        assert!(status.contains("10 дн."), "статус: {}", status);
    }

    #[test]
    fn реферальная_ссылка_содержит_id() {
        emoji::reset_for_tests();
        let mut user = Value::obj();
        user.set("user_id", 777i64);
        let menu = invite(&user, "my_bot");
        assert!(menu.text.contains("t.me/my_bot?start=ref_777"));
    }

    #[test]
    fn админ_панель_показывает_статистику() {
        emoji::reset_for_tests();
        let store = test_store();
        let menu = admin(&store);
        assert!(menu.text.contains("Пользователей"));
        assert_eq!(menu.banner, Some("admin"));
    }

    fn test_config() -> Config {
        Config {
            bot_token: "t".into(),
            admin_ids: vec![1],
            database_path: std::path::PathBuf::from("db.json"),
            encryption_key: "k".into(),
            api_base: "https://api.telegram.org".into(),
            assets_dir: std::path::PathBuf::from("/tmp"),
            max_export_mb: 48,
            max_single_media_mb: 35,
            max_import_file_mb: 200,
            max_imported_media_mb: 100,
            custom_emoji: true,
            verbose: false,
            temp_dir: std::path::PathBuf::from("/tmp"),
        }
    }

    fn test_store() -> Store {
        let path = std::env::temp_dir().join("savemod_menus_test.json");
        let _ = std::fs::remove_file(&path);
        Store::open(&path, "k")
    }
}
