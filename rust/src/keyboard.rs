//! Сборка inline-клавиатур и защита от «мёртвых» кнопок.
//!
//! Два правила, зашитых в код:
//!   1. `callback_data` всегда отдельным полем и не совпадает с текстом
//!      (раньше он подменялся подписью, и все кнопки переставали работать);
//!   2. если Telegram не даёт кастомные эмодзи, клавиатура отправляется
//!      без иконок — меню не должно пропадать.

use crate::emoji::{button, escape};
use crate::json::Value;

pub const PRICE_7: i64 = 25;
pub const PRICE_30: i64 = 75;
pub const PRICE_90: i64 = 150;
pub const PRICE_PREMIUM: i64 = 300;

pub const ARCHIVE_TYPES: [&str; 10] = [
    "text",
    "photo",
    "video",
    "audio",
    "document",
    "animation",
    "video_note",
    "voice",
    "location",
    "sticker",
];

pub fn type_label(kind: &str) -> String {
    match kind {
        "text" => "💬 Текст".to_string(),
        "photo" => "🖼 Фото".to_string(),
        "video" => "🎬 Видео".to_string(),
        "audio" => "🎵 Аудио".to_string(),
        "document" => "📎 Файлы".to_string(),
        "animation" => "🎞 Гиф".to_string(),
        "video_note" => "🔘 Кружки".to_string(),
        "voice" => "🎤 Голос".to_string(),
        "location" => "📍 Гео".to_string(),
        "sticker" => "✨ Стикеры".to_string(),
        other => other.to_string(),
    }
}

/// Строитель клавиатуры: ряды кнопок.
pub struct Keyboard {
    rows: Vec<Vec<Value>>,
    current: Vec<Value>,
}

impl Keyboard {
    pub fn new() -> Self {
        Keyboard {
            rows: Vec::new(),
            current: Vec::new(),
        }
    }

    pub fn add(mut self, button: Value) -> Self {
        if let Some(callback_data) = button.get("callback_data").and_then(|v| v.as_str()) {
            let text = button.get_str("text");
            let length = callback_data.len();
            assert!(
                (1..=64).contains(&length),
                "callback_data кнопки «{}» неверной длины: {}",
                text,
                length
            );
            assert!(
                callback_data != text,
                "callback_data кнопки «{}» совпадает с текстом",
                text
            );
        }
        self.current.push(button);
        self
    }

    /// Закрывает текущий ряд. Пустой ряд не добавляется, чтобы
    /// двойной `.row()` не создавал дыр в клавиатуре.
    pub fn row(mut self) -> Self {
        if !self.current.is_empty() {
            self.rows.push(std::mem::take(&mut self.current));
        }
        self
    }

    pub fn build(mut self) -> Value {
        // Кнопки, добавленные после последнего .row(), образуют свой ряд,
        // а не приклеиваются к предыдущему.
        if !self.current.is_empty() {
            self.rows.push(std::mem::take(&mut self.current));
        }
        Value::from_pairs(vec![(
            "inline_keyboard",
            Value::arr(self.rows.into_iter().map(Value::arr).collect()),
        )])
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
    }
}

fn back(label: &str) -> Value {
    button(label, "back_main", "back", "primary")
}

/// Главное меню.
///
/// Исходная раскладка была 2+2+1 (Тарифы|Пригласить друга,
/// Подключение|Создать бота, Настройки). Кнопку «Создать бота» убрали,
/// поэтому осталось 2+1+1: «Настройки» по-прежнему отдельным рядом,
/// как и в присланном дизайне.
pub fn main(user_id: i64, is_admin: bool) -> Value {
    let mut keyboard = Keyboard::new()
        .add(button("Тарифы", "tariffs", "gem", "primary"))
        .add(button("Пригласить друга", "invite", "folder", "primary"))
        .row()
        .add(button("Подключение", "connect", "plug", "primary"))
        .row()
        .add(button("Настройки", "settings", "gear", "primary"))
        .row();

    if is_admin {
        keyboard = keyboard
            .add(button("Админ-панель", "admin", "crown", "danger"))
            .row();
    }

    let _ = user_id;
    keyboard.build()
}

pub fn settings(notify_deleted: bool, notify_edited: bool) -> Value {
    Keyboard::new()
        .add(button(
            &format!("Удалённые: {}", if notify_deleted { "ВКЛ" } else { "ВЫКЛ" }),
            "toggle_deleted",
            if notify_deleted { "check" } else { "cross" },
            if notify_deleted { "success" } else { "danger" },
        ))
        .row()
        .add(button(
            &format!("Изменённые: {}", if notify_edited { "ВКЛ" } else { "ВЫКЛ" }),
            "toggle_edited",
            if notify_edited { "check" } else { "cross" },
            if notify_edited { "success" } else { "danger" },
        ))
        .row()
        .add(back("Назад"))
        .build()
}

pub fn connect() -> Value {
    Keyboard::new()
        .add(button("Проверить подключение", "check_connection", "check", "success"))
        .row()
        .add(button("Как подключить", "connect_help", "clip", "primary"))
        .row()
        .add(back("Назад"))
        .build()
}

pub fn tariffs() -> Value {
    Keyboard::new()
        .add(button(
            &format!("7 дней — {} ⭐", PRICE_7),
            "buy_sub_7",
            "gem",
            "primary",
        ))
        .add(button(
            &format!("30 дней — {} ⭐", PRICE_30),
            "buy_sub_30",
            "gem",
            "primary",
        ))
        .row()
        .add(button(
            &format!("90 дней — {} ⭐", PRICE_90),
            "buy_sub_90",
            "gem",
            "primary",
        ))
        .row()
        .add(button("Премиум", "buy_sub_premium", "star", "success"))
        .row()
        .add(back("Назад"))
        .build()
}

pub fn premium() -> Value {
    Keyboard::new()
        .add(button(
            &format!("Купить Премиум — {} ⭐", PRICE_PREMIUM),
            "buy_sub_premium",
            "star",
            "success",
        ))
        .row()
        .add(button("Все тарифы", "tariffs", "gem", "primary"))
        .row()
        .add(back("Назад"))
        .build()
}

pub fn invite() -> Value {
    Keyboard::new()
        .add(button("Поделиться ссылкой", "share_link", "folder", "success"))
        .row()
        .add(button("Открыть в Telegram", "invite_url", "plug", "primary"))
        .row()
        .add(back("Назад"))
        .build()
}

pub fn archive_types(selected: &[String]) -> Value {
    let mut keyboard = Keyboard::new();

    for chunk in ARCHIVE_TYPES.chunks(2) {
        for kind in chunk {
            let active = selected.iter().any(|item| item == kind);
            keyboard = keyboard.add(button(
                &type_label(kind),
                &format!("arch_toggle_{}", kind),
                if active { "check" } else { "cross" },
                if active { "success" } else { "primary" },
            ));
        }
        keyboard = keyboard.row();
    }

    keyboard
        .add(button("Отменить", "arch_cancel", "cross", "danger"))
        .add(button("Начать экспорт", "arch_start", "check", "success"))
        .build()
}

pub fn admin() -> Value {
    Keyboard::new()
        .add(button("Выдать подписку", "admin_give_sub", "check", "success"))
        .add(button("Забрать подписку", "admin_take_sub", "cross", "danger"))
        .row()
        .add(button("Выдать/снять Премиум", "admin_toggle_prem", "star", "success"))
        .add(button("Проверить юзера", "admin_check_user", "user", "primary"))
        .row()
        .add(button("Забанить", "admin_ban_user", "cross", "danger"))
        .add(button("Разбанить", "admin_unban_user", "check", "success"))
        .row()
        .add(button("Рассылка всем", "admin_broadcast", "plug", "primary"))
        .row()
        .add(back("Назад в меню"))
        .build()
}

pub fn chats(titles: &[(i64, String)]) -> Value {
    let mut keyboard = Keyboard::new();
    for (chat_id, title) in titles.iter().take(35) {
        let mut label: String = title.chars().take(48).collect();
        if label.is_empty() {
            label = format!("Чат {}", chat_id);
        }
        keyboard = keyboard.add(button(
            &label,
            &format!("arch_sel_{}", chat_id),
            "user",
            "primary",
        ));
    }
    keyboard.row().add(back("Назад")).build()
}

pub fn empty_chats() -> Value {
    Keyboard::new()
        .add(button("Подключить бота", "connect", "plug", "primary"))
        .row()
        .add(back("Назад"))
        .build()
}

pub fn import_wait() -> Value {
    Keyboard::new()
        .add(button("Отмена", "archive_import_cancel", "cross", "danger"))
        .build()
}

pub fn import_result(ok: bool) -> Value {
    let mut keyboard = Keyboard::new();
    if !ok {
        keyboard = keyboard.add(button("Повторить импорт", "archive_import_again", "arrow", "primary")).row();
    }
    keyboard.add(back("Назад в меню")).build()
}

pub fn admin_back() -> Value {
    Keyboard::new().add(back("Назад в админ-панель")).build()
}

/// Копия клавиатуры без кастомных эмодзи.
pub fn strip_emojis(markup: &Value) -> Value {
    let rows = markup
        .get_arr("inline_keyboard")
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let buttons = row
                        .as_array()
                        .map(|buttons| {
                            buttons
                                .iter()
                                .map(|button| {
                                    let mut copy = Value::obj();
                                    if let Some(map) = button.as_object() {
                                        for (key, value) in map {
                                            if key != "icon_custom_emoji_id" {
                                                copy.set(key, value.clone());
                                            }
                                        }
                                    }
                                    copy
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Value::arr(buttons)
                })
                .collect()
        })
        .unwrap_or_default();

    Value::from_pairs(vec![("inline_keyboard", Value::arr(rows))])
}

/// Текст подсказки для меню настроек (используется в Menus).
pub fn escape_title(title: &str) -> String {
    escape(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emoji;

    #[test]
    fn главное_меню_содержит_нужные_кнопки() {
        emoji::reset_for_tests();
        let markup = main(1, false);
        let serialized = markup.to_string();
        for expected in ["tariffs", "invite", "connect", "settings"] {
            assert!(
                serialized.contains(&format!("\"{}\"", expected)),
                "нет кнопки {}",
                expected
            );
        }
    }

    #[test]
    fn в_меню_больше_нет_кнопки_создать_бота() {
        emoji::reset_for_tests();
        let serialized = main(1, false).to_string();
        assert!(
            !serialized.contains("create_bot"),
            "кнопка «Создать бота» осталась в меню"
        );
        assert!(
            !serialized.contains("Создать бота"),
            "подпись «Создать бота» осталась в меню"
        );
    }

    #[test]
    fn раскладка_ряды_на_месте() {
        emoji::reset_for_tests();
        let markup = main(1, false);
        let rows = markup.get_arr("inline_keyboard").unwrap();
        assert_eq!(rows.len(), 3, "должно быть 3 ряда");
        // Тарифы | Пригласить друга
        assert_eq!(rows[0].as_array().unwrap().len(), 2);
        // Подключение
        assert_eq!(rows[1].as_array().unwrap().len(), 1);
        // Настройки
        assert_eq!(rows[2].as_array().unwrap().len(), 1);
    }

    #[test]
    fn админ_видит_четвёртый_ряд() {
        emoji::reset_for_tests();
        let markup = main(1, true);
        let rows = markup.get_arr("inline_keyboard").unwrap();
        assert_eq!(rows.len(), 4);
        assert!(markup.to_string().contains("admin"));
    }

    #[test]
    fn кнопки_имеют_иконки() {
        emoji::reset_for_tests();
        let serialized = main(1, false).to_string();
        assert!(serialized.contains("icon_custom_emoji_id"));
    }

    #[test]
    fn strip_emojis_убирает_иконки_но_оставляет_кнопки() {
        emoji::reset_for_tests();
        let markup = main(1, false);
        let stripped = strip_emojis(&markup).to_string();
        assert!(!stripped.contains("icon_custom_emoji_id"));
        assert!(stripped.contains("tariffs"));
    }

    #[test]
    fn callback_data_не_равен_тексту() {
        emoji::reset_for_tests();
        let serialized = main(1, false).to_string();
        assert!(!serialized.contains("\"callback_data\":\"Тарифы\""));
        assert!(serialized.contains("\"callback_data\":\"tariffs\""));
    }

    #[test]
    fn архивные_типы_переключаются() {
        emoji::reset_for_tests();
        let selected = vec!["text".to_string(), "photo".to_string()];
        let serialized = archive_types(&selected).to_string();
        assert!(serialized.contains("arch_toggle_text"));
        assert!(serialized.contains("arch_start"));
    }

    #[test]
    #[should_panic(expected = "совпадает с текстом")]
    fn паникует_на_подменённом_callback() {
        Keyboard::new()
            .add(Value::from_pairs(vec![
                ("text", Value::str("Проверка")),
                ("callback_data", Value::str("Проверка")),
            ]))
            .build();
    }

    #[test]
    #[should_panic(expected = "неверной длины")]
    fn паникует_на_длинном_callback() {
        Keyboard::new()
            .add(Value::from_pairs(vec![
                ("text", Value::str("X")),
                ("callback_data", Value::str("a".repeat(70))),
            ]))
            .build();
    }
}

