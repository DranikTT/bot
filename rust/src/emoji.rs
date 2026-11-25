//! Кастомные (премиум) эмодзи Telegram через HTML.
//!
//! Тексты уходят с `parse_mode=HTML` и тегом
//! `<tg-emoji emoji-id="…">💎</tg-emoji>`, кнопки получают тот же ID
//! полем `icon_custom_emoji_id`.
//!
//! **Почему нужен фолбэк.** Telegram отдаёт кастомные эмодзи только ботам,
//! у которых владелец имеет Premium либо бот купил юзернейм на Fragment.
//! Без права любой такой тег даёт `400: DOCUMENT_INVALID` и отклоняет
//! **всё сообщение** — именно это роняло меню. Поэтому здесь:
//!
//! 1. `allowed` — глобальный флаг: как только Telegram один раз отказал,
//!    эмодзи больше не отправляются, сразу подставляется обычный;
//! 2. `strip_html` — снимает обёртку с уже готового текста для повторной
//!    отправки без потери содержания;
//! 3. `CUSTOM_EMOJI=off` в конфиге отключает их принудительно.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::json::Value;

/// Разрешены ли кастомные эмодзи в этом запуске.
static ALLOWED: AtomicBool = AtomicBool::new(true);

/// Отключено пользователем через `CUSTOM_EMOJI=off`.
static DISABLED_BY_CONFIG: AtomicBool = AtomicBool::new(false);

pub fn disable_by_config() {
    DISABLED_BY_CONFIG.store(true, Ordering::Relaxed);
    ALLOWED.store(false, Ordering::Relaxed);
}

pub fn is_allowed() -> bool {
    ALLOWED.load(Ordering::Relaxed) && !DISABLED_BY_CONFIG.load(Ordering::Relaxed)
}

pub fn is_disabled_by_config() -> bool {
    DISABLED_BY_CONFIG.load(Ordering::Relaxed)
}

/// Telegram отказал — больше не пытаемся отправлять эмодзи.
pub fn mark_not_allowed() {
    if ALLOWED.swap(false, Ordering::Relaxed) {
        crate::api::log::warn(
            "Кастомные эмодзи недоступны боту (нужен Telegram Premium у владельца \
             или юзернейм, купленный на Fragment). Переключаюсь на обычные эмодзи.",
        );
    }
}

/// Сбрасывает изученное ограничение (тесты).
pub fn reset_for_tests() {
    ALLOWED.store(true, Ordering::Relaxed);
    DISABLED_BY_CONFIG.store(false, Ordering::Relaxed);
}

/// Карта «имя → ID кастомного эмодзи».
///
/// ID взяты из прежних версий проекта, поэтому картинки те же, что и раньше.
/// Неизвестный ID даёт DOCUMENT_INVALID, так что карта — часть контракта,
/// и тест сверяет её с оригиналом.
pub fn ids() -> BTreeMap<&'static str, &'static str> {
    let mut map = BTreeMap::new();
    map.insert("wave", "5994750571041525522"); // 👋
    map.insert("back", "5877468380125990242"); // 🔙
    map.insert("check", "5776375003280838798"); // ✅
    map.insert("cross", "5778527486270770928"); // ❌
    map.insert("gear", "5877260593903175642"); // ⚙️
    map.insert("plug", "5877465816030515018"); // 🔌 / 📢
    map.insert("folder", "5877482652302315100"); // 📁 / 👥
    map.insert("user", "5877477244938489129"); // 👤
    map.insert("gem", "6028530359975548369"); // 💎
    map.insert("star", "5879785854284599288"); // ⭐️ / 🎁
    map.insert("crown", "5807868868886009920"); // 👑
    map.insert("arrow", "5875008416132370818"); // 🔽
    map.insert("shield", "5778423822940114949"); // 🛡
    map.insert("clip", "5877495434124988415"); // 📎
    map.insert("pin", "5796440171364749940"); // 📌
    map.insert("stats", "5931472654660800739"); // 📊
    map
}

pub fn id_of(name: &str) -> Option<&'static str> {
    ids().get(name).copied()
}

/// ID похож на настоящий: 16–20 цифр, не все нули.
pub fn is_valid_id(value: &str) -> bool {
    let length_ok = (15..=20).contains(&value.len());
    length_ok && value.chars().all(|c| c.is_ascii_digit()) && value.chars().any(|c| c != '0')
}

/// Оборачивает эмодзи в `<tg-emoji>`, если это разрешено.
pub fn html(name: &str, fallback: &str) -> String {
    match id_of(name) {
        Some(id) if is_allowed() => format!("<tg-emoji emoji-id=\"{}\">{}</tg-emoji>", id, fallback),
        _ => fallback.to_string(),
    }
}

/// Снимает обёртку `<tg-emoji>`, оставляя обычный эмодзи.
pub fn strip_html(text: &str) -> String {
    if !text.contains("<tg-emoji") {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("<tg-emoji") {
        out.push_str(&rest[..start]);
        // пропускаем открывающий тег целиком
        match rest[start..].find('>') {
            Some(offset) => {
                rest = &rest[start + offset + 1..];
            }
            None => {
                // битый тег — отдаём как есть
                out.push_str(&rest[start..]);
                return out;
            }
        }
        // копируем содержимое до закрывающего тега
        match rest.find("</tg-emoji>") {
            Some(offset) => {
                out.push_str(&rest[..offset]);
                rest = &rest[offset + "</tg-emoji>".len()..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Кнопка с кастомным эмодзи и стилем.
pub fn button(text: &str, callback_data: &str, emoji_name: &str, style: &str) -> Value {
    let mut button = Value::from_pairs(vec![
        ("text", Value::str(text)),
        ("callback_data", Value::str(callback_data)),
        ("style", Value::str(style)),
    ]);
    if let Some(id) = id_of(emoji_name) {
        if is_allowed() {
            button.set("icon_custom_emoji_id", id);
        }
    }
    button
}

/// Кнопка-ссылка.
pub fn url_button(text: &str, url: &str, emoji_name: &str, style: &str) -> Value {
    let mut button = Value::from_pairs(vec![
        ("text", Value::str(text)),
        ("url", Value::str(url)),
        ("style", Value::str(style)),
    ]);
    if let Some(id) = id_of(emoji_name) {
        if is_allowed() {
            button.set("icon_custom_emoji_id", id);
        }
    }
    button
}

/// Экранирование текста для parse_mode=HTML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn все_id_валидны() {
        for (name, id) in ids() {
            assert!(is_valid_id(id), "некорректный ID у «{}»: {}", name, id);
        }
        assert!(ids().len() >= 14, "слишком мало эмодзи: {}", ids().len());
    }

    #[test]
    fn id_совпадают_с_прежней_версией() {
        // значения из git: menuKeys.js и тексты menus.js
        let expected = [
            ("wave", "5994750571041525522"),
            ("back", "5877468380125990242"),
            ("check", "5776375003280838798"),
            ("cross", "5778527486270770928"),
            ("gear", "5877260593903175642"),
            ("plug", "5877465816030515018"),
            ("folder", "5877482652302315100"),
            ("user", "5877477244938489129"),
            ("gem", "6028530359975548369"),
            ("star", "5879785854284599288"),
            ("crown", "5807868868886009920"),
            ("arrow", "5875008416132370818"),
            ("shield", "5778423822940114949"),
            ("clip", "5877495434124988415"),
        ];
        for (name, id) in expected {
            assert_eq!(id_of(name), Some(id), "разошёлся ID «{}»", name);
        }
    }

    #[test]
    fn html_формирует_тег() {
        reset_for_tests();
        assert_eq!(
            html("gem", "💎"),
            "<tg-emoji emoji-id=\"6028530359975548369\">💎</tg-emoji>"
        );
    }

    #[test]
    fn html_без_права_даёт_обычный_эмодзи() {
        reset_for_tests();
        mark_not_allowed();
        assert_eq!(html("gem", "💎"), "💎");
        reset_for_tests();
    }

    #[test]
    fn strip_html_убирает_теги() {
        reset_for_tests();
        let text = format!("Привет {} и {}", html("wave", "👋"), html("gem", "💎"));
        let plain = strip_html(&text);
        assert!(!plain.contains("tg-emoji"), "тег остался: {}", plain);
        assert!(plain.contains("👋"), "эмодзи пропал: {}", plain);
        assert!(plain.contains("💎"), "эмодзи пропал: {}", plain);
    }

    #[test]
    fn strip_html_не_трогает_обычный_текст() {
        assert_eq!(strip_html("обычный текст"), "обычный текст");
    }

    #[test]
    fn strip_html_переживает_битый_тег() {
        let broken = "начало <tg-emoji emoji-id=\"1\">💎 без закрывающего";
        // главное — не паника и сохранение содержания
        let result = strip_html(broken);
        assert!(result.contains("💎"));
    }

    #[test]
    fn кнопка_содержит_иконку_и_стиль() {
        reset_for_tests();
        let button = button("Тест", "test_cb", "gem", "primary");
        assert_eq!(button.get_str("callback_data"), "test_cb");
        assert_eq!(button.get_str("icon_custom_emoji_id"), "6028530359975548369");
        assert_eq!(button.get_str("style"), "primary");
        assert_eq!(button.get_str("text"), "Тест");
    }

    #[test]
    fn кнопка_не_путает_текст_и_callback() {
        reset_for_tests();
        let button = button("Тарифы", "tariffs", "gem", "primary");
        // регрессия: раньше callback_data подменялся текстом кнопки
        assert_ne!(button.get_str("callback_data"), button.get_str("text"));
        assert_eq!(button.get_str("callback_data"), "tariffs");
    }

    #[test]
    fn экранирование() {
        assert_eq!(escape("<b>&\"x\"</b>"), "&lt;b&gt;&amp;&quot;x&quot;&lt;/b&gt;");
    }
}
