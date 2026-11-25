//! Обработчики обновлений Telegram.
//!
//! Здесь сходятся все ветки: команды, кнопки, оплата, Business-события,
//! админка и экспорт. Логика отделена от сети — `Handlers::handle` получает
//! разобранный JSON и возвращает, что делать, поэтому тестируется на фейковом
//! сервере и юнит-тестами без живого Telegram.

use crate::api::Api;
use crate::archive;
use crate::config::Config;
use crate::emoji::{button, escape, html};
use crate::json::Value;
use crate::keyboard::Keyboard;
use crate::menus::{self, format_time, Menu};
use crate::sender::Sender;
use crate::store::{now, NewMessage, Store};

pub const PRICE_7: i64 = 25;
pub const PRICE_30: i64 = 75;
pub const PRICE_90: i64 = 150;
pub const PRICE_PREMIUM: i64 = 300;

/// Все доступные типы медиа для архива.
pub const MEDIA_TYPES: [&str; 9] = [
    "text",
    "photo",
    "video",
    "voice",
    "video_note",
    "animation",
    "sticker",
    "document",
    "location",
];

pub struct Handlers<'a> {
    pub api: &'a Api,
    pub store: &'a mut Store,
    pub config: &'a Config,
    pub sender: &'a Sender,
    pub bot_username: String,
}

/// Что показать пользователю.
enum Reply {
    Menu(Menu),
    Text(String),
}

impl<'a> Handlers<'a> {
    pub fn new(
        api: &'a Api,
        store: &'a mut Store,
        config: &'a Config,
        sender: &'a Sender,
        bot_username: &str,
    ) -> Self {
        Handlers {
            api,
            store,
            config,
            sender,
            bot_username: bot_username.to_string(),
        }
    }

    /// Сохраняет изменения базы (вызывается после каждого обновления).
    pub fn flush(&mut self) {
        self.store.flush();
    }

    // ------------------------------------------------------------- вход

    pub fn handle(&mut self, update: &Value) {
        // Telegram Business: подключение и события чатов
        if let Some(connection) = update.get("business_connection") {
            self.on_business_connection(connection);
        }
        if let Some(message) = update.get("business_message") {
            self.on_business_message(message);
        }
        if let Some(edited) = update.get("edited_business_message") {
            self.on_business_edited(edited);
        }
        if let Some(deleted) = update.get("deleted_business_messages") {
            self.on_business_deleted(deleted);
        }

        if let Some(callback) = update.get("callback_query") {
            self.on_callback(callback);
            return;
        }
        if let Some(message) = update.get("message") {
            self.on_message(message);
            return;
        }
        if let Some(checkout) = update.get("pre_checkout_query") {
            self.on_pre_checkout(checkout);
            return;
        }
        if let Some(payment) = update.get("message") {
            if payment.get("successful_payment").is_some() {
                self.on_payment(payment);
            }
        }
        if let Some(inline) = update.get("inline_query") {
            self.on_inline(inline);
        }
    }

    // ------------------------------------------------------- сообщения

    fn on_message(&mut self, message: &Value) {
        let Some(from) = message.get("from") else {
            return;
        };
        let user_id = from.get_i64("id", 0);
        if user_id == 0 {
            return;
        }

        let username = from.get_str("username");
        let first_name = from.get_str("first_name");
        let referred_by = parse_referral(message);

        let (user, created, referrer) =
            self.store
                .create_user(user_id, &username, &first_name, referred_by);

        if user.get_bool("is_banned", false) {
            crate::api::log::debug(format!("[message] пользователь {} забанен", user_id));
            return;
        }
        self.store.flush();

        let chat_id = message.get("chat").map(|chat| chat.get_i64("id", 0)).unwrap_or(0);
        if chat_id == 0 {
            return;
        }

        if let Some(payment) = message.get("successful_payment") {
            self.apply_payment(user_id, payment);
            self.show_main(chat_id, user_id, message);
            return;
        }

        let text = message.get_str("text");
        let document = message.get("document");

        // импорт архива: ждём файл
        if self.awaiting_import(user_id) {
            if let Some(document) = document {
                self.import_document(user_id, chat_id, document);
                return;
            }
            if !text.is_empty() {
                self.sender.message(
                    self.api,
                    chat_id,
                    &format!(
                        "{} Отправьте <b>result.json</b> или ZIP-архив Telegram Desktop.",
                        html("clip", "📥")
                    ),
                    None,
                );
                return;
            }
        }

        // админ ждёт ввода (ID пользователя / дни / текст рассылки)
        if self.config.is_admin(user_id) && !text.is_empty() {
            if self.handle_admin_input(user_id, chat_id, &text) {
                return;
            }
        }

        // команда /start с рефералом
        if text.starts_with("/start") {
            if created {
                if let Some(referrer_id) = referrer {
                    self.notify_referrer(referrer_id, &first_name);
                }
            }
            crate::api::log::debug(format!("[/start] пользователь {}", user_id));
            let user = self.store.user(user_id).unwrap_or(user);
            self.reply_with_menu(chat_id, menus::main(&user, self.store, self.config));
            return;
        }

        if text.starts_with("/help") {
            self.help(chat_id);
            return;
        }
        if text.starts_with("/archive") {
            self.archive_menu(chat_id, user_id);
            return;
        }
        if text.starts_with("/import") {
            self.store.update_user(user_id, "awaiting_import", Value::Bool(true));
            self.store.flush();
            self.reply_with_menu(chat_id, menus::import_wait());
            return;
        }

        // reply-keyboard команды внутри Business-чата
        self.handle_business_command(user_id, chat_id, &text);
    }

    fn help(&mut self, chat_id: i64) {
        let text = format!(
            "{} <b>Команды</b>\n\n\
             /start — главное меню\n\
             /help — эта справка\n\
             /archive — экспорт переписки\n\
             /import — импорт Telegram Desktop\n\n\
             <b>Управление перехватом</b> (в Business-чате):\n\
             <code>.nah</code> — отключить перехват\n\
             <code>.love</code> — включить перехват\n\
             <code>.bypass</code> — скрыть перехват для собеседника\n\
             <code>.mute</code> / <code>.unmute</code> — уведомления",
            html("shield", "🛡")
        );
        let keyboard = Keyboard::new().add(button("Назад", "back_main", "arrow", "danger")).build();
        self.reply_with_menu(chat_id, Menu::new(text, Some(keyboard)));
    }

    fn show_main(&mut self, chat_id: i64, user_id: i64, _message: &Value) {
        let user = self.store.user(user_id).unwrap_or(Value::obj());
        self.reply_with_menu(chat_id, menus::main(&user, self.store, self.config));
    }

    /// Показывает меню в ответ на сообщение пользователя (ничего не удаляя).
    fn reply_with_menu(&mut self, chat_id: i64, menu: Menu) {
        self.sender.show(self.api, chat_id, &menu, None, None);
    }

    // -------------------------------------------------------- кнопки

    fn on_callback(&mut self, callback: &Value) {
        let Some(from) = callback.get("from") else {
            return;
        };
        let user_id = from.get_i64("id", 0);
        let data = callback.get_str("data");
        let callback_id = callback.get_str("id");
        let message = callback.get("message");

        let chat_id = message
            .and_then(|message| message.get("chat"))
            .map(|chat| chat.get_i64("id", 0))
            .unwrap_or(0);
        let message_id = message.map(|message| message.get_i64("message_id", 0)).unwrap_or(0);

        self.api.answer_callback_query(&callback_id, None, false);

        let user = match self.store.user(user_id) {
            Some(user) => user,
            None => {
                self.store.create_user(user_id, &from.get_str("username"), &from.get_str("first_name"), None);
                self.store.flush();
                self.store.user(user_id).unwrap_or(Value::obj())
            }
        };

        if user.get_bool("is_banned", false) {
            self.api.answer_callback_query(&callback_id, Some("🚫 Доступ ограничен"), true);
            return;
        }

        let is_admin = self.config.is_admin(user_id);
        // именно здесь раньше был прокол: сравнение только с "admin_"
        // пропускало саму кнопку "admin" и пускало всех в админку
        let admin_screen = data == "admin" || data.starts_with("admin_");
        if admin_screen && !is_admin {
            self.api.answer_callback_query(&callback_id, Some("⛔ Нет прав"), true);
            return;
        }

        let reply = self.route_button(&data, &user, user_id, chat_id, is_admin, message);

        match reply {
            Some(Reply::Menu(menu)) => {
                self.sender.show(
                    self.api,
                    chat_id,
                    &menu,
                    if message_id > 0 { Some(message_id) } else { None },
                    None,
                );
            }
            Some(Reply::Text(text)) => {
                self.sender.show(
                    self.api,
                    chat_id,
                    &Menu::new(text, None),
                    if message_id > 0 { Some(message_id) } else { None },
                    None,
                );
            }
            None => {}
        }
        self.store.flush();
    }

    fn route_button(
        &mut self,
        data: &str,
        user: &Value,
        user_id: i64,
        chat_id: i64,
        _is_admin: bool,
        message: Option<&Value>,
    ) -> Option<Reply> {
        match data {
            "back_main" => Some(Reply::Menu(menus::main(user, self.store, self.config))),
            "settings" => Some(Reply::Menu(menus::settings(user))),
            "tariffs" => Some(Reply::Menu(menus::tariffs(user, self.config))),
            "premium" => Some(Reply::Menu(menus::premium(user, self.store, self.config))),
            "invite" => Some(Reply::Menu(menus::invite(user, &self.bot_username))),
            "connect" => Some(Reply::Menu(menus::connect(&self.bot_username))),

            "toggle_deleted" => {
                let value = !user.get_bool("notify_deleted", true);
                self.store.update_user(user_id, "notify_deleted", Value::Bool(value));
                let user = self.store.user(user_id).unwrap_or(user.clone());
                Some(Reply::Menu(menus::settings(&user)))
            }
            "toggle_edited" => {
                let value = !user.get_bool("notify_edited", true);
                self.store.update_user(user_id, "notify_edited", Value::Bool(value));
                let user = self.store.user(user_id).unwrap_or(user.clone());
                Some(Reply::Menu(menus::settings(&user)))
            }

            "check_connection" => {
                let connected = self.store.connection_of(user_id).is_some();
                Some(Reply::Text(if connected {
                    format!(
                        "{} <b>Подключение активно</b>\n\nПерехват работает. Управление — командами в Business-чате.",
                        html("check", "✅")
                    )
                } else {
                    format!(
                        "{} <b>Подключение не найдено</b>\n\nПодключите бота в настройках Telegram Business, затем повторите проверку.",
                        html("cross", "❌")
                    )
                }))
            }
            "connect_help" => Some(Reply::Menu(menus::connect(&self.bot_username))),

            // ---- оплата
            "buy_sub_7" => {
                self.send_invoice(user_id, chat_id, "sub_7", "Подписка на 7 дней", 7, PRICE_7, message);
                None
            }
            "buy_sub_30" => {
                self.send_invoice(user_id, chat_id, "sub_30", "Подписка на 30 дней", 30, PRICE_30, message);
                None
            }
            "buy_sub_90" => {
                self.send_invoice(user_id, chat_id, "sub_90", "Подписка на 90 дней", 90, PRICE_90, message);
                None
            }
            "buy_sub_premium" => {
                self.send_invoice(user_id, chat_id, "premium", "Премиум на 365 дней", 365, PRICE_PREMIUM, message);
                None
            }

            "share_link" => {
                let link = format!("https://t.me/{}?start=ref_{}", self.bot_username, user_id);
                Some(Reply::Text(format!(
                    "{} Перешлите другу это сообщение — ссылка уже внутри:\n\n{}",
                    html("folder", "👥"),
                    escape(&link)
                )))
            }
            "invite_url" => {
                let link = format!("https://t.me/{}?start=ref_{}", self.bot_username, user_id);
                
                Some(Reply::Text(format!("<code>{}</code>", escape(&link))))
            }

            // ---- архив
            "arch_cancel" => Some(Reply::Menu(menus::main(user, self.store, self.config))),
            "archive_import_cancel" | "archive_import_again" => {
                self.store.update_user(user_id, "awaiting_import", Value::Bool(false));
                Some(Reply::Menu(menus::main(user, self.store, self.config)))
            }
            "arch_start" => {
                self.export_archive(user_id, chat_id);
                None
            }

            // ---- админка
            "admin" => Some(Reply::Menu(menus::admin(self.store))),
            "admin_back" => Some(Reply::Menu(menus::admin(self.store))),
            _ => {
                if let Some(kind) = data.strip_prefix("arch_toggle_") {
                    self.toggle_archive_type(user_id, kind);
                    return None;
                }
                if let Some(chat) = data.strip_prefix("arch_sel_") {
                    if let Ok(chat_id) = chat.parse::<i64>() {
                        self.open_archive_types(user_id, chat_id);
                    }
                    return None;
                }
                if let Some(action) = data.strip_prefix("admin_") {
                    return self.admin_action(action, user, chat_id);
                }
                crate::api::log::debug(format!("неизвестная кнопка: {}", data));
                None
            }
        }
    }

    // -------------------------------------------------------- оплата

    fn send_invoice(
        &mut self,
        _user_id: i64,
        chat_id: i64,
        payload: &str,
        title: &str,
        days: i64,
        stars: i64,
        _message: Option<&Value>,
    ) {
        let description = format!("{} — {} дней доступа", title, days);

        if let Err(error) = self.api.send_invoice(
            chat_id,
            title,
            &description,
            &format!("sub|{}", payload),
            stars,
        ) {
            crate::api::log::error(format!("[sendInvoice] {}", error));
        }
    }

    fn on_pre_checkout(&mut self, query: &Value) {
        let id = query.get_str("id");
        let payload = query.get_str("invoice_payload");
        let total = query.get_i64("total_amount", 0);

        let expected = match payload.split('|').nth(1).unwrap_or("") {
            "sub_7" => PRICE_7,
            "sub_30" => PRICE_30,
            "sub_90" => PRICE_90,
            "premium" => PRICE_PREMIUM,
            _ => 0,
        };

        // сумма должна совпасть с тарифом (в звёздах)
        if expected == 0 || total != expected {
            crate::api::log::warn(format!(
                "pre_checkout: ожидалось {}, пришло {} — отклонено",
                expected, total
            ));
            self.api.reject_pre_checkout(&id, "Некорректный платёж.");
            return;
        }
        self.api.answer_pre_checkout_query(&id);
    }

    fn on_payment(&mut self, message: &Value) {
        let Some(from) = message.get("from") else {
            return;
        };
        let user_id = from.get_i64("id", 0);
        let Some(payment) = message.get("successful_payment") else {
            return;
        };
        self.apply_payment(user_id, payment);
    }

    fn apply_payment(&mut self, user_id: i64, payment: &Value) {
        let payload = payment.get_str("invoice_payload");
        let Some(kind) = payload.split('|').nth(1) else {
            return;
        };

        let (days, premium) = match kind {
            "sub_7" => (7, false),
            "sub_30" => (30, false),
            "sub_90" => (90, false),
            "premium" => (365, true),
            _ => return,
        };

        self.store.add_days(user_id, days, premium);
        self.store.flush();

        let Some(user) = self.store.user(user_id) else {
            return;
        };
        let text = format!(
            "{} <b>Оплата получена!</b>\n\n\
             {} Доступ продлён на <b>{} дн.</b>\n\
             {} Действует до: <b>{}</b>",
            html("check", "✅"),
            html("gem", "💎"),
            days,
            html("star", "⭐️"),
            format_time(user.get_i64("subscription_until", 0)),
        );
        self.sender.message(self.api, user_id, &text, None);
        crate::api::log::debug(format!("оплата: пользователь {}, тариф {}", user_id, kind));
    }

    // -------------------------------------------------------- Business

    fn on_business_connection(&mut self, connection: &Value) {
        let connection_id = connection.get_str("id");
        let owner_id = connection
            .get("user")
            .map(|user| user.get_i64("id", 0))
            .unwrap_or(0);
        let enabled = connection.get_bool("is_enabled", false);

        if !connection_id.is_empty() && owner_id != 0 {
            // владелец мог подключить бота, не заходя в /start
            if self.store.user(owner_id).is_none() {
                let username = connection
                    .get("user")
                    .map(|user| user.get_str("username"))
                    .unwrap_or_default();
                let first_name = connection
                    .get("user")
                    .map(|user| user.get_str("first_name"))
                    .unwrap_or_default();
                self.store
                    .create_user(owner_id, &username, &first_name, None);
            }
            self.store.save_connection(owner_id, &connection_id, enabled);
            self.store.flush();

            let message = if enabled {
                format!("{} <b>Подключение активно</b>", html("check", "✅"))
            } else {
                format!("{} <b>Подключение отключено</b>", html("cross", "❌"))
            };
            self.sender.message(self.api, owner_id, &message, None);
        }
    }

    fn on_business_message(&mut self, message: &Value) {
        let connection_id = message.get_str("business_connection_id");
        let Some(owner_id) = self.store.owner_of_connection(&connection_id) else {
            return;
        };
        if self.store.is_banned(owner_id) {
            return;
        }

        let chat = message.get("chat").cloned().unwrap_or(Value::obj());
        let chat_id = chat.get_i64("id", 0);
        let message_id = message.get_i64("message_id", 0);
        let text = message.get_str("text");
        let sender = message.get("from").cloned().unwrap_or(Value::obj());
        let outgoing = chat_id == owner_id;

        let from_owner = sender.get_i64("id", 0) == owner_id;

        // владелец вводит команды управления прямо в чате
        if from_owner && text.trim().starts_with('.') {
            self.handle_business_command(owner_id, chat_id, &text);
            return;
        }

        // точка/запятая от собеседника — перехват сгорающих медиа
        if !outgoing && matches!(text.trim(), "." | ",") {
            self.forward_self_destruct(owner_id, message, &chat);
            return;
        }

        let (media_type, file_id, file_name) = detect_media(message);

        self.store.save_message(NewMessage {
            owner_id,
            chat_id,
            message_id,
            sender_id: Some(sender.get_i64("id", 0)),
            sender_name: sender.get_str("first_name"),
            sender_username: sender.get_str("username"),
            peer_name: crate::store::peer_title(&chat),
            peer_username: chat.get_str("username"),
            text: if text.is_empty() && media_type == "text" { String::new() } else { text.clone() },
            media_type: Some(media_type.clone()),
            file_id,
            file_name,
            is_outgoing: outgoing,
            date: message.get_i64("date", now()),
        });
        self.store.flush();

        if !outgoing {
            self.notify_about(message, owner_id, "новое сообщение");
        }
    }

    fn on_business_edited(&mut self, message: &Value) {
        let connection_id = message.get_str("business_connection_id");
        let Some(owner_id) = self.store.owner_of_connection(&connection_id) else {
            return;
        };
        if !self.store.user(owner_id).map(|user| user.get_bool("notify_edited", true)).unwrap_or(true) {
            return;
        }

        let chat_id = message.get("chat").map(|chat| chat.get_i64("id", 0)).unwrap_or(0);
        let message_id = message.get_i64("message_id", 0);
        self.store
            .save_edited_text(owner_id, chat_id, message_id, &message.get_str("text"));
        self.store.flush();
    }

    fn on_business_deleted(&mut self, deleted: &Value) {
        let connection_id = deleted.get_str("business_connection_id");
        let Some(owner_id) = self.store.owner_of_connection(&connection_id) else {
            return;
        };
        if !self.store.user(owner_id).map(|user| user.get_bool("notify_deleted", true)).unwrap_or(true) {
            return;
        }

        let chat_id = deleted.get("chat").map(|chat| chat.get_i64("id", 0)).unwrap_or(0);
        let ids: Vec<i64> = deleted
            .get_arr("message_ids")
            .map(|ids| ids.iter().map(|value| value.as_i64().unwrap_or(0)).collect())
            .unwrap_or_default();

        self.store.mark_deleted(owner_id, chat_id, &ids);
        self.store.flush();

        let text = format!(
            "{} <b>Сообщения удалены</b>\n\nВ чате <code>{}</code> удалено сообщений: <b>{}</b>",
            html("cross", "🗑"),
            chat_id,
            ids.len()
        );
        self.sender.message(self.api, owner_id, &text, None);
    }

    /// Перехват сгорающих медиа (сообщение из «.» или «,»).
    fn forward_self_destruct(&mut self, owner_id: i64, message: &Value, chat: &Value) {
        let Some(media) = message.get("self_destruct_media") else {
            return;
        };
        let _ = media;

        let chat_id = chat.get_i64("id", 0);
        let text = format!(
            "{} <b>Перехвачено сгорающее медиа</b>\n\nСобеседник: <b>{}</b>",
            html("check", "🔥"),
            escape(&crate::store::peer_title(chat))
        );
        self.sender.message(self.api, owner_id, &text, None);
        let _ = chat_id;
    }

    fn notify_about(&mut self, message: &Value, owner_id: i64, label: &str) {
        let chat = message.get("chat").cloned().unwrap_or(Value::obj());
        let sender = message.get("from").cloned().unwrap_or(Value::obj());
        let text = message.get_str("text");

        let mut body = format!(
            "{} <b>{}</b>\n\n{}",
            html("clip", "📩"),
            escape(label),
            escape(&crate::store::peer_title(&chat))
        );
        if !sender.get_str("first_name").is_empty() {
            body.push_str(&format!("\nАвтор: {}", escape(&sender.get_str("first_name"))));
        }
        if !text.is_empty() {
            body.push_str(&format!("\n\n<blockquote>{}</blockquote>", escape(&text)));
        }
        self.sender.message(self.api, owner_id, &body, None);
    }

    fn notify_referrer(&mut self, referrer_id: i64, friend_name: &str) {
        let name = if friend_name.trim().is_empty() {
            "друг".to_string()
        } else {
            escape(friend_name)
        };
        let text = format!(
            "{} <b>Новый реферал!</b>\n\n\
             Пользователь {} присоединился.\n\
             Вам начислено <b>+7 дней</b> подписки.",
            html("folder", "👥"),
            name
        );
        self.sender.message(self.api, referrer_id, &text, None);
    }

    // --------------------------------------------------------- команды

    fn handle_business_command(&mut self, owner_id: i64, chat_id: i64, text: &str) {
        let command = text.trim().to_lowercase();
        let reply = match command.as_str() {
            ".nah" => {
                self.store.update_user(owner_id, "intercept", Value::Bool(false));
                Some("Перехват отключён")
            }
            ".love" => {
                self.store.update_user(owner_id, "intercept", Value::Bool(true));
                Some("Перехват включён")
            }
            ".bypass" => {
                let value = !self.store.user(owner_id).map(|user| user.get_bool("bypass", false)).unwrap_or(false);
                self.store.update_user(owner_id, "bypass", Value::Bool(value));
                Some(if value { "Перехват скрыт" } else { "Перехват снова виден" })
            }
            ".mute" => {
                self.store.update_user(owner_id, "notify_deleted", Value::Bool(false));
                self.store.update_user(owner_id, "notify_edited", Value::Bool(false));
                Some("Уведомления отключены")
            }
            ".unmute" => {
                self.store.update_user(owner_id, "notify_deleted", Value::Bool(true));
                self.store.update_user(owner_id, "notify_edited", Value::Bool(true));
                Some("Уведомления включены")
            }
            _ => None,
        };

        if let Some(answer) = reply {
            self.store.flush();
            self.sender.message(self.api, chat_id, &escape(answer), None);
        }
    }

    /// Возвращает true, если ввод был админ-командой и его обработали.
    fn handle_admin_input(&mut self, admin_id: i64, chat_id: i64, text: &str) -> bool {
        let pending = self
            .store
            .user(admin_id)
            .map(|user| user.get_str("admin_pending"))
            .unwrap_or_default();

        if pending == "broadcast" {
            self.store.update_user(admin_id, "admin_pending", Value::str(""));
            let targets = crate::store::broadcast_targets(self.store);
            let mut sent = 0usize;
            for target in &targets {
                if self
                    .api
                    .send_message(*target, &escape(text), None, None)
                    .is_ok()
                {
                    sent += 1;
                }
                // бережём лимиты Telegram
                std::thread::sleep(std::time::Duration::from_millis(35));
            }
            let report = format!(
                "{} <b>Рассылка завершена</b>\n\nДоставлено: <b>{}</b> из <b>{}</b>",
                html("check", "✅"),
                sent,
                targets.len()
            );
            self.sender.message(self.api, chat_id, &report, None);
            self.store.flush();
            return true;
        }

        if pending.is_empty() {
            return false;
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        let Some(target) = parts.first().and_then(|value| value.parse::<i64>().ok()) else {
            self.sender.message(
                self.api,
                chat_id,
                &format!("{} Нужен числовой ID. Попробуйте ещё раз или /start.", html("cross", "❌")),
                None,
            );
            return true;
        };

        self.store.update_user(admin_id, "admin_pending", Value::str(""));

        let answer = match pending.as_str() {
            "give_sub" => {
                let days = parts.get(1).and_then(|value| value.parse::<i64>().ok()).unwrap_or(30);
                self.store.add_days(target, days, false);
                format!("✅ Выдано <b>{}</b> дн. пользователю <code>{}</code>", days, target)
            }
            "take_sub" => {
                self.store.reset_subscription(target);
                format!("✅ Снята подписка с <code>{}</code>", target)
            }
            "toggle_prem" => {
                let current = self
                    .store
                    .user(target)
                    .map(|user| user.get_bool("is_premium", false))
                    .unwrap_or(false);
                self.store.set_premium(target, !current);
                format!(
                    "{} Премиум для <code>{}</code>: <b>{}</b>",
                    html("star", "⭐️"),
                    target,
                    if current { "снят" } else { "выдан" }
                )
            }
            "check_user" => match self.store.user(target) {
                Some(user) => format!(
                    "{} <b>Пользователь</b> <code>{}</code>\n\n                     Имя: {}\n                     Админ: {}\n                     Бан: {}\n                     Рефералов: {}\n\
                     Подписка: {}",
                    html("user", "👤"),
                    target,
                    escape(&user.get_str("first_name")),
                    if self.config.is_admin(target) { "да" } else { "нет" },
                    if user.get_bool("is_banned", false) { "да" } else { "нет" },
                    user.get_i64("referrals_count", 0),
                    escape(&menus::subscription_status(&user, self.config)),
                ),
                None => format!("❌ Пользователь <code>{}</code> не найден", target),
            },
            "ban_user" => {
                self.store.set_ban(target, true);
                format!("🚫 Забанен: <code>{}</code>", target)
            }
            "unban_user" => {
                self.store.set_ban(target, false);
                format!("✅ Разбанен: <code>{}</code>", target)
            }
            _ => return false,
        };

        self.sender.message(self.api, chat_id, &answer, None);
        self.store.flush();
        true
    }

    /// Экспорт выбранного чата в HTML и отправка файлом.
    fn export_archive(&mut self, user_id: i64, chat_id: i64) {
        let target = self
            .store
            .user(user_id)
            .map(|user| user.get_i64("archive_chat", 0))
            .unwrap_or(0);

        if target == 0 {
            self.sender.message(
                self.api,
                chat_id,
                &format!("{} Сначала выберите чат через /archive.", html("cross", "❌")),
                None,
            );
            return;
        }

        let selected: Vec<String> = self
            .store
            .user(user_id)
            .and_then(|user| user.get("archive_types").map(|value| value.str_array()))
            .unwrap_or_else(|| MEDIA_TYPES.iter().map(|item| item.to_string()).collect());

        if selected.is_empty() {
            self.sender.message(
                self.api,
                chat_id,
                &format!("{} Не выбрано ни одного типа сообщений.", html("cross", "❌")),
                None,
            );
            return;
        }

        let title = self
            .store
            .chats_of(user_id)
            .into_iter()
            .find(|(id, _)| *id == target)
            .map(|(_, title)| title)
            .unwrap_or_else(|| format!("Чат {}", target));

        let messages = self.store.messages_of(user_id, target);
        let result = archive::export(&messages, target, &title, &selected);

        if result.count == 0 {
            self.sender.message(
                self.api,
                chat_id,
                &format!(
                    "{} В этом чате нет сообщений выбранных типов.\nПопробуйте /archive и выберите другие.",
                    html("cross", "❌")
                ),
                None,
            );
            return;
        }

        let bytes = result.html.into_bytes();
        let limit_bytes = self.config.max_export_mb * 1024 * 1024;
        if bytes.len() as i64 > limit_bytes {
            self.sender.message(
                self.api,
                chat_id,
                &format!(
                    "{} Архив слишком большой ({} МБ при лимите {} МБ).<br>Выберите меньше типов сообщений.",
                    html("cross", "❌"),
                    bytes.len() / 1024 / 1024,
                    self.config.max_export_mb
                ),
                None,
            );
            return;
        }

        let file_name = format!("chat_{}_archive.html", target);
        let caption = format!(
            "{} <b>Архив готов</b>\n\nЧат: <b>{}</b>\nСообщений: <b>{}</b>\nРазмер: <b>{} КБ</b>",
            html("folder", "📁"),
            escape(&title),
            result.count,
            bytes.len() / 1024
        );

        if let Err(error) = self.api.send_document(
            chat_id,
            &file_name,
            bytes,
            &caption,
            Some(&Keyboard::new().add(button("Назад в меню", "back_main", "arrow", "primary")).build()),
        ) {
            crate::api::log::error(format!("[sendDocument] {}", error));
        }
    }

    // ---------------------------------------------------------- архив

    fn archive_menu(&mut self, chat_id: i64, user_id: i64) {
        let chats = self.store.chats_of(user_id);
        let menu = if chats.is_empty() {
            menus::empty_chats()
        } else {
            menus::chats_list(&chats)
        };
        self.reply_with_menu(chat_id, menu);
    }

    fn open_archive_types(&mut self, user_id: i64, chat_id: i64) {
        let chats = self.store.chats_of(user_id);
        let title = chats
            .iter()
            .find(|(id, _)| *id == chat_id)
            .map(|(_, title)| title.clone())
            .unwrap_or_else(|| format!("Чат {}", chat_id));

        let selected: Vec<String> = self
            .store
            .user(user_id)
            .and_then(|user| user.get("archive_types").map(|value| value.str_array()))
            .unwrap_or_else(|| MEDIA_TYPES.iter().map(|item| item.to_string()).collect());

        self.store.update_user(user_id, "archive_chat", Value::Int(chat_id));
        self.store.flush();

        let menu = menus::archive_types(chat_id, &title, &selected);
        self.sender.message(self.api, user_id, &menu.text, menu.keyboard.as_ref());
    }

    fn toggle_archive_type(&mut self, user_id: i64, kind: &str) {
        let mut selected: Vec<String> = self
            .store
            .user(user_id)
            .and_then(|user| user.get("archive_types").map(|value| value.str_array()))
            .unwrap_or_else(|| MEDIA_TYPES.iter().map(|item| item.to_string()).collect());

        if let Some(position) = selected.iter().position(|item| item == kind) {
            selected.remove(position);
        } else {
            selected.push(kind.to_string());
        }

        self.store.update_user(
            user_id,
            "archive_types",
            Value::Array(selected.iter().map(|item| Value::str(item)).collect()),
        );

        let chat_id = self
            .store
            .user(user_id)
            .map(|user| user.get_i64("archive_chat", 0))
            .unwrap_or(0);
        if chat_id == 0 {
            return;
        }

        let title = self
            .store
            .chats_of(user_id)
            .into_iter()
            .find(|(id, _)| *id == chat_id)
            .map(|(_, title)| title)
            .unwrap_or_else(|| format!("Чат {}", chat_id));

        let menu = menus::archive_types(chat_id, &title, &selected);
        if let Some(keyboard) = menu.keyboard {
            self.api.edit_message_text(user_id, 0, &menu.text, Some(&keyboard)).ok();
        }
        self.store.flush();
    }

    fn awaiting_import(&self, user_id: i64) -> bool {
        self.store
            .user(user_id)
            .map(|user| user.get_bool("awaiting_import", false))
            .unwrap_or(false)
    }

    fn import_document(&mut self, user_id: i64, chat_id: i64, document: &Value) {
        let file_id = document.get_str("file_id");
        let file_name = document.get_str("file_name");
        let size = document.get_i64("file_size", 0);
        let limit = self.config.max_import_file_mb * 1024 * 1024;

        if size > limit {
            self.sender.message(
                self.api,
                chat_id,
                &format!(
                    "{} Файл больше {} МБ — уменьшите выгрузку.",
                    html("cross", "❌"),
                    self.config.max_import_file_mb
                ),
                None,
            );
            return;
        }

        let bytes = match self.api.download_file(&file_id) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.sender.message(
                    self.api,
                    chat_id,
                    &format!("{} Не удалось скачать файл: {}", html("cross", "❌"), escape(&error.to_string())),
                    None,
                );
                return;
            }
        };

        let json_bytes = if file_name.to_lowercase().ends_with(".zip") {
            match archive::extract_zip(&bytes, &self.config.temp_dir.join("import")) {
                Ok(path) => match std::fs::read(&path) {
                    Ok(data) => data,
                    Err(error) => {
                        self.sender.message(self.api, chat_id, &format!("Ошибка чтения: {}", error), None);
                        return;
                    }
                },
                Err(error) => {
                    self.sender.message(self.api, chat_id, &format!("{} {}", html("cross", "❌"), escape(&error)), None);
                    return;
                }
            }
        } else {
            bytes
        };

        let json_text = String::from_utf8_lossy(&json_bytes).to_string();
        match archive::import_desktop(&json_text, user_id, self.store) {
            Ok((imported_chat, title, count)) => {
                self.store.update_user(user_id, "awaiting_import", Value::Bool(false));
                self.store.flush();
                let text = format!(
                    "{} <b>Импорт завершён</b>\n\n\
                     Чат: <b>{}</b>\n\
                     Добавлено сообщений: <b>{}</b>\n\n\
                     Откройте /archive, чтобы выгрузить его.",
                    html("check", "✅"),
                    escape(&title),
                    count
                );
                let keyboard = Keyboard::new()
                    .add(button("К архиву", "back_main", "folder", "primary"))
                    .build();
                self.sender
                    .message(self.api, chat_id, &text, Some(&keyboard));
                crate::api::log::debug(format!("импорт: чат {} ({}), {} сообщений", imported_chat, title, count));
            }
            Err(error) => {
                self.sender.message(
                    self.api,
                    chat_id,
                    &format!("{} Импорт не удался: {}", html("cross", "❌"), escape(&error)),
                    None,
                );
            }
        }
    }

    // --------------------------------------------------------- админка

    fn admin_action(&mut self, action: &str, user: &Value, chat_id: i64) -> Option<Reply> {
        let _ = user;
        match action {
            "stats" => Some(Reply::Menu(menus::admin(self.store))),
            "broadcast" => {
                self.store.update_user(chat_id, "awaiting_broadcast", Value::Bool(true));
                Some(Reply::Text(format!(
                    "{} Отправьте текст рассылки.",
                    html("clip", "📢")
                )))
            }
            "users" => {
                let users = self.store.all_users();
                let mut lines = String::new();
                for user in users.iter().take(30) {
                    let banned = if user.get_bool("is_banned", false) { " 🚫" } else { "" };
                    lines.push_str(&format!(
                        "\n• {} — <code>{}</code>{}",
                        escape(&user.get_str("first_name")),
                        user.get_i64("user_id", 0),
                        banned
                    ));
                }
                Some(Reply::Text(format!(
                    "{} <b>Пользователи</b> (первые 30)\n{}",
                    html("user", "👥"),
                    lines
                )))
            }
            "give_sub" | "take_sub" | "toggle_prem" | "check_user" | "ban_user" | "unban_user" => {
                self.store
                    .update_user(chat_id, "admin_pending", Value::str(action));
                self.store.flush();
                let hint = match action {
                    "give_sub" => "ID и количество дней: <code>123456789 30</code>",
                    "take_sub" => "у кого забрать подписку — отправьте ID",
                    "toggle_prem" => "кому выдать/снять Премиум — отправьте ID",
                    "check_user" => "кого проверить — отправьте ID",
                    "ban_user" => "кого забанить — отправьте ID",
                    _ => "кого разбанить — отправьте ID",
                };
                Some(Reply::Text(format!(
                    "{} <b>Действие</b>: {}\n\nОтправьте: {}",
                    html("gear", "⚙️"),
                    action,
                    hint
                )))
            }
            _ => None,
        }
    }

    fn on_inline(&mut self, query: &Value) {
        let id = query.get_str("id");
        let results = vec![Value::from_pairs(vec![
            ("type", Value::str("article")),
            ("id", Value::str("1")),
            (
                "title",
                Value::str("SaveMod — управление перехватом в Telegram Business"),
            ),
            (
                "input_message_content",
                Value::from_pairs(vec![(
                    "message_text",
                    Value::str("Подключите бота в Telegram Business и держите переписку под контролем."),
                )]),
            ),
        ])];
        if let Err(error) = self.api.answer_inline_query(&id, results) {
            crate::api::log::debug(format!("answerInlineQuery: {}", error));
        }
    }
}

fn parse_referral(message: &Value) -> Option<i64> {
    let text = message.get_str("text");
    let payload = text.strip_prefix("/start")?.trim();
    let payload = payload.strip_prefix("ref_")?;
    payload.parse::<i64>().ok()
}

/// Тип медиа, file_id и имя файла.
fn detect_media(message: &Value) -> (String, Option<String>, Option<String>) {
    if let Some(photo) = message.get_arr("photo").and_then(|sizes| sizes.last()) {
        return ("photo".to_string(), Some(photo.get_str("file_id")), None);
    }
    for (key, kind) in [
        ("video", "video"),
        ("voice", "voice"),
        ("video_note", "video_note"),
        ("animation", "animation"),
        ("sticker", "sticker"),
        ("audio", "audio"),
        ("document", "document"),
    ] {
        if let Some(media) = message.get(key) {
            let file_name = media.get_str("file_name");
            return (
                kind.to_string(),
                Some(media.get_str("file_id")),
                if file_name.is_empty() { None } else { Some(file_name) },
            );
        }
    }
    if message.get("location").is_some() {
        return ("location".to_string(), None, None);
    }
    ("text".to_string(), None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn реферал_разбирается() {
        let mut message = Value::obj();
        message.set("text", "/start ref_12345");
        assert_eq!(parse_referral(&message), Some(12345));

        message.set("text", "/start");
        assert_eq!(parse_referral(&message), None);

        message.set("text", "привет");
        assert_eq!(parse_referral(&message), None);
    }

    #[test]
    fn определяет_тип_медиа() {
        let mut message = Value::obj();
        message.set(
            "photo",
            Value::Array(vec![Value::from_pairs(vec![("file_id", Value::str("small"))]), Value::from_pairs(vec![("file_id", Value::str("big"))])]),
        );
        let (kind, file_id, _) = detect_media(&message);
        assert_eq!(kind, "photo");
        assert_eq!(file_id, Some("big".to_string()));

        let mut video = Value::obj();
        video.set("video", Value::from_pairs(vec![("file_id", Value::str("v1"))]));
        assert_eq!(detect_media(&video).0, "video");

        let mut text = Value::obj();
        text.set("text", "привет");
        assert_eq!(detect_media(&text).0, "text");
    }

    #[test]
    fn цены_верные() {
        assert_eq!(PRICE_7, 25);
        assert_eq!(PRICE_30, 75);
        assert_eq!(PRICE_90, 150);
        assert_eq!(PRICE_PREMIUM, 300);
    }
}
