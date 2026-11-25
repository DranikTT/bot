//! Отправка меню с баннером и фолбэком.
//!
//! Пути к картинкам — как в конфиге:
//!   main → assets/mainmenu.jpg
//!   settings → assets/settings.jpg
//!   subscription → assets/subscription.jpg
//!   admin → assets/adminmenu.jpg
//!
//! Три уровня защиты (исправляет DOCUMENT_INVALID):
//!   1. файл проверяется на существование и сигнатуру JPEG/PNG — «не картинка»
//!      отвергается ещё до запроса;
//!   2. если Telegram всё равно отклонил фото — меню уходит текстом
//!      с рабочими кнопками, а не падает;
//!   3. кастомные эмодзи отключаются, если Telegram их не даёт.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::api::Api;
use crate::config::Config;
use crate::json::Value;
use crate::menus::Menu;

pub struct Sender {
    banners: BTreeMap<&'static str, PathBuf>,
}

impl Sender {
    pub fn new(config: &Config) -> Self {
        let mut banners = BTreeMap::new();
        // имена ровно как в конфиге пользователя
        banners.insert("main", config.assets_dir.join("mainmenu.jpg"));
        banners.insert("settings", config.assets_dir.join("settings.jpg"));
        banners.insert("subscription", config.assets_dir.join("subscription.jpg"));
        banners.insert("admin", config.assets_dir.join("adminmenu.jpg"));
        Sender { banners }
    }

    /// Файл существует и похож на картинку.
    pub fn banner_is_valid(&self, key: &str) -> bool {
        let Some(path) = self.banners.get(key) else {
            return false;
        };
        if !path.is_file() {
            return false;
        }
        matches!(std::fs::read(path), Ok(bytes) if is_image_bytes(&bytes))
    }

    /// Показывает меню: по возможности правит текущее сообщение,
    /// иначе отправляет новое.
    pub fn show(
        &self,
        api: &Api,
        chat_id: i64,
        menu: &Menu,
        message_id: Option<i64>,
        business_connection_id: Option<&str>,
    ) {
        // Меню с баннером всегда показываем фотографией: подпись с картинкой
        // нельзя «дописать» в текстовое сообщение, поэтому при переходе
        // между экранами отправляем новое фото, а старое убираем.
        let wants_photo = menu
            .banner
            .map(|banner| self.banner_is_valid(banner))
            .unwrap_or(false);

        if wants_photo {
            if let Some(id) = message_id {
                // старое сообщение — наше меню, его место занимает новое
                api.delete_message(chat_id, id);
            }
            self.send_new(api, chat_id, menu, business_connection_id);
            return;
        }

        // Текстовые экраны правим на месте
        if let Some(id) = message_id {
            if self.edit_existing(api, chat_id, id, menu) {
                return;
            }
        }
        self.send_new(api, chat_id, menu, business_connection_id);
    }

    fn edit_existing(&self, api: &Api, chat_id: i64, message_id: i64, menu: &Menu) -> bool {
        match api.edit_message_text(chat_id, message_id, &menu.text, menu.keyboard.as_ref()) {
            Ok(_) => return true,
            Err(error) if error.is_not_editable() => {
                // «there is no text in the message to edit» — под кнопками
                // старое меню с фото, править нечего. Отправляем новое.
                crate::api::log::debug(format!(
                    "editMessageText: сообщение не редактируется ({}), отправляю новое",
                    error.description
                ));
                return false;
            }
            Err(error) => {
                crate::api::log::debug(format!("editMessageText: {}", error));
            }
        }

        // Текст не влез в старое сообщение (например, там была подпись к фото) —
        // отправим новое, чтобы пользователь точно увидел ответ.
        false
    }

    fn send_new(&self, api: &Api, chat_id: i64, menu: &Menu, business_connection_id: Option<&str>) {
        if let Some(banner) = menu.banner {
            if self.banner_is_valid(banner) {
                let path = self.banners.get(banner).unwrap();
                match api.send_photo(chat_id, path, &menu.text, menu.keyboard.as_ref()) {
                    Ok(_) => return,
                    Err(error) => {
                        // Даже если Telegram отверг картинку, меню обязано
                        // уйти: пользователь не должен остаться без ответа.
                        crate::api::log::warn(format!(
                            "[sendMenu] баннер «{}» не отправлен: {}",
                            banner, error
                        ));
                    }
                }
            } else {
                crate::api::log::debug(format!(
                    "[sendMenu] баннер «{}» отсутствует или не картинка — отправляю текстом",
                    banner
                ));
            }
        }

        // send_message сам снимет эмодзи, если у бота нет права
        if let Err(error) = api.send_message(chat_id, &menu.text, menu.keyboard.as_ref(), business_connection_id)
        {
            crate::api::log::error(format!("[sendMenu] не удалось отправить меню: {}", error));
        }
    }

    /// Простое сообщение с клавиатурой.
    pub fn message(&self, api: &Api, chat_id: i64, text: &str, keyboard: Option<&Value>) {
        if let Err(error) = api.send_message(chat_id, text, keyboard, None) {
            crate::api::log::error(format!("[sendMessage] {}", error));
        }
    }

    /// Сообщение в Business-чат (ответ от имени владельца).
    pub fn business_message(
        &self,
        api: &Api,
        chat_id: i64,
        text: &str,
        keyboard: Option<&Value>,
        connection_id: &str,
    ) {
        if let Err(error) = api.send_message(chat_id, text, keyboard, Some(connection_id)) {
            crate::api::log::error(format!("[business sendMessage] {}", error));
        }
    }
}

/// Проверка по сигнатуре: JPEG, PNG, GIF, WEBP.
pub fn is_image_bytes(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    let jpeg = bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF;
    let png = &bytes[..8] == b"\x89PNG\r\n\x1a\n";
    let gif = &bytes[..4] == b"GIF8";
    let webp = &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP";
    jpeg || png || gif || webp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::PathBuf;

    fn config_with_assets(dir: PathBuf) -> Config {
        Config {
            bot_token: "t".into(),
            admin_ids: vec![],
            database_path: PathBuf::from("db.json"),
            encryption_key: "k".into(),
            api_base: "http://127.0.0.1:1".into(),
            assets_dir: dir,
            max_export_mb: 48,
            max_single_media_mb: 35,
            max_import_file_mb: 200,
            max_imported_media_mb: 100,
            custom_emoji: true,
            verbose: false,
            temp_dir: std::env::temp_dir(),
        }
    }

    #[test]
    fn имена_баннеров_ровно_из_конфига() {
        let sender = Sender::new(&config_with_assets(PathBuf::from("/app/assets")));
        assert!(sender.banners["main"].ends_with("mainmenu.jpg"));
        assert!(sender.banners["settings"].ends_with("settings.jpg"));
        assert!(sender.banners["subscription"].ends_with("subscription.jpg"));
        assert!(sender.banners["admin"].ends_with("adminmenu.jpg"));
    }

    #[test]
    fn отсутствующий_файл_невалиден() {
        let dir = std::env::temp_dir().join("savemod_no_such_dir");
        let _ = std::fs::remove_dir_all(&dir);
        let sender = Sender::new(&config_with_assets(dir));
        assert!(!sender.banner_is_valid("main"));
    }

    #[test]
    fn не_картинка_невалидна() {
        let dir = std::env::temp_dir().join("savemod_bad_banner");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mainmenu.jpg"), "это не картинка вообще".as_bytes()).unwrap();

        let sender = Sender::new(&config_with_assets(dir));
        assert!(!sender.banner_is_valid("main"), "текстовый файл принят за картинку");
    }

    #[test]
    fn настоящий_jpeg_валиден() {
        let dir = std::env::temp_dir().join("savemod_good_banner");
        std::fs::create_dir_all(&dir).unwrap();
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.extend_from_slice(&[0u8; 64]);
        std::fs::write(dir.join("mainmenu.jpg"), &jpeg).unwrap();

        let sender = Sender::new(&config_with_assets(dir));
        assert!(sender.banner_is_valid("main"));
    }

    #[test]
    fn определяет_сигнатуры() {
        assert!(is_image_bytes(&[0xFF, 0xD8, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(is_image_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00"));
        assert!(is_image_bytes(b"GIF89a\x00\x00\x00\x00\x00\x00\x00"));
        assert!(is_image_bytes(b"RIFF\x00\x00\x00\x00WEBP"));
        assert!(!is_image_bytes(b"<html>not image"));
        assert!(!is_image_bytes(b""));
    }

    #[test]
    fn реальные_баннеры_в_репозитории_валидны() {
        // запуск из rust/ или из корня
        let root = crate::config::Config::load()
            .map(|config| config.assets_dir)
            .unwrap_or_else(|_| PathBuf::from("../assets"));

        if !root.is_dir() {
            return; // тестовое окружение без ассетов
        }
        let sender = Sender::new(&config_with_assets(root));
        for key in ["main", "settings", "subscription", "admin"] {
            assert!(sender.banner_is_valid(key), "баннер «{}» невалиден", key);
        }
    }
}
