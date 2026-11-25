//! Клиент Bot API.
//!
//! Тонкая обёртка над HTTP: только те методы, которые реально нужны боту.
//! Именно поэтому доступны поля `style` и `icon_custom_emoji_id` у кнопок —
//! их нет в популярных обёртках, но Telegram их поддерживает.

use crate::emoji;
use crate::http::{encode_path, HttpClient, Response};
use crate::json::Value;

pub struct Api {
    http: HttpClient,
    base: String,
    token: String,
}

/// Ошибка Telegram (код + человекочитаемое описание).
#[derive(Debug)]
pub struct ApiError {
    pub code: i64,
    pub description: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.description)
    }
}

impl ApiError {
    /// Лимит запросов — нужно подождать.
    pub fn is_rate_limit(&self) -> bool {
        self.code == 429
    }

    pub fn retry_after_seconds(&self) -> u64 {
        // Telegram присылает «Too Many Requests: retry after N»
        let lower = self.description.to_lowercase();
        if let Some(index) = lower.find("retry after ") {
            let tail = &self.description[index + 12..];
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            return digits.parse().unwrap_or(1);
        }
        1
    }

    /// Кастомные эмодзи недоступны боту (нет Premium у владельца
    /// или не куплен юзернейм на Fragment).
    pub fn is_custom_emoji_error(&self) -> bool {
        if self.code != 400 {
            return false;
        }
        let upper = self.description.to_uppercase();
        upper.contains("DOCUMENT_INVALID")
            || upper.contains("CUSTOM_EMOJI")
            || upper.contains("BUTTON_ICON")
    }

    /// Сообщение нельзя отредактировать: под кнопками фото, а не текст,
    /// либо его уже нет. Значит, надо отправить новое.
    pub fn is_not_editable(&self) -> bool {
        let lower = self.description.to_lowercase();
        lower.contains("there is no text in the message to edit")
            || lower.contains("message can't be edited")
            || lower.contains("message to edit not found")
            || lower.contains("message is not modified")
    }
}

impl Api {
    pub fn new(token: &str, api_base: &str) -> Self {
        let base = format!("{}/bot{}", api_base.trim_end_matches('/'), token);
        Api {
            http: HttpClient::new(api_base),
            base,
            token: token.to_string(),
        }
    }

    /// Вызов метода Bot API. Текст ошибки Telegram превращается в ApiError.
    pub fn call(&self, method: &str, payload: &Value) -> Result<Value, ApiError> {
        let url = format!("{}/{}", self.base, method);
        let body = payload.to_string();
        let response = self
            .http
            .post_json(&url, &body, 60)
            .map_err(|error| ApiError {
                code: 0,
                description: format!("{}: {}", method, error),
            })?;
        self.parse(method, response)
    }

    fn parse(&self, method: &str, response: Response) -> Result<Value, ApiError> {
        let parsed = Value::parse(&response.text()).map_err(|error| ApiError {
            code: 0,
            description: format!("{}: некорректный ответ ({}): {}", method, error, response.status),
        })?;

        if parsed.get_bool("ok", false) {
            return Ok(parsed.get("result").cloned().unwrap_or(Value::Null));
        }

        Err(ApiError {
            code: parsed.get_i64("error_code", response.status as i64),
            description: format!(
                "[{}] {}",
                method,
                parsed.get_str("description")
            ),
        })
    }

    /// Вызов multipart-метода (фото, документ).
    fn call_multipart(
        &self,
        method: &str,
        fields: Vec<(String, String)>,
        file: Option<(&str, String, Vec<u8>, String)>,
    ) -> Result<Value, ApiError> {
        let url = format!("{}/{}", self.base, method);
        let file_ref = file
            .as_ref()
            .map(|(field, name, content, mime)| (*field, name.as_str(), content.as_slice(), mime.as_str()));

        let response = self
            .http
            .post_multipart(&url, &fields, file_ref, 120)
            .map_err(|error| ApiError {
                code: 0,
                description: format!("{}: {}", method, error),
            })?;
        self.parse(method, response)
    }

    // ------------------------------------------------------------- методы

    pub fn get_me(&self) -> Result<Value, ApiError> {
        self.call("getMe", &Value::obj())
    }

    pub fn delete_webhook(&self) -> Result<Value, ApiError> {
        self.call("deleteWebhook", &Value::obj())
    }

    pub fn set_my_commands(&self, commands: Vec<(&str, &str)>) -> Result<Value, ApiError> {
        let items: Vec<Value> = commands
            .into_iter()
            .map(|(command, description)| {
                Value::from_pairs(vec![
                    ("command", Value::str(command)),
                    ("description", Value::str(description)),
                ])
            })
            .collect();
        self.call("setMyCommands", &Value::from_pairs(vec![("commands", Value::arr(items))]))
    }

    pub fn get_updates(&self, offset: i64, timeout: i64) -> Result<Vec<Value>, ApiError> {
        let payload = Value::from_pairs(vec![
            ("offset", Value::Int(offset)),
            ("timeout", Value::Int(timeout)),
            (
                "allowed_updates",
                Value::arr(
                    [
                        "message",
                        "callback_query",
                        "inline_query",
                        "pre_checkout_query",
                        "business_connection",
                        "business_message",
                        "edited_business_message",
                        "deleted_business_messages",
                    ]
                    .iter()
                    .map(|item| Value::str(*item))
                    .collect(),
                ),
            ),
        ]);

        let url = format!("{}/getUpdates", self.base);
        // long polling: таймаут HTTP должен быть больше серверного
        let response = self
            .http
            .post_json(&url, &payload.to_string(), timeout + 20)
            .map_err(|error| ApiError {
                code: 0,
                description: format!("getUpdates: {}", error),
            })?;
        let parsed = self.parse("getUpdates", response)?;
        Ok(parsed.as_array().cloned().unwrap_or_default())
    }

    /// Отправка сообщения с фолбэком: если Telegram не даёт кастомные
    /// эмодзи, тот же текст уходит с обычными, а не падает с ошибкой.
    pub fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&Value>,
        business_connection_id: Option<&str>,
    ) -> Result<Value, ApiError> {
        match self.send_message_once(chat_id, text, keyboard, business_connection_id, true) {
            Ok(value) => Ok(value),
            Err(error) if error.is_custom_emoji_error() => {
                emoji::mark_not_allowed();
                log::warn("[sendMessage] кастомные эмодзи недоступны, отправляю с обычными");
                self.send_message_once(chat_id, text, keyboard, business_connection_id, false)
            }
            Err(error) => Err(error),
        }
    }

    fn send_message_once(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&Value>,
        business_connection_id: Option<&str>,
        with_emoji: bool,
    ) -> Result<Value, ApiError> {
        let mut payload = Value::obj();
        payload.set("chat_id", chat_id);
        payload.set(
            "text",
            if with_emoji { text.to_string() } else { emoji::strip_html(text) },
        );
        payload.set("parse_mode", "HTML");
        payload.set(
            "link_preview_options",
            Value::from_pairs(vec![("is_disabled", Value::Bool(true))]),
        );
        if let Some(markup) = keyboard {
            let markup = if with_emoji {
                markup.clone()
            } else {
                crate::keyboard::strip_emojis(markup)
            };
            payload.set("reply_markup", markup);
        }
        if let Some(connection) = business_connection_id {
            payload.set("business_connection_id", connection.to_string());
        }
        self.call("sendMessage", &payload)
    }

    pub fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&Value>,
    ) -> Result<Value, ApiError> {
        match self.edit_once(chat_id, message_id, text, keyboard, true) {
            Ok(value) => Ok(value),
            Err(error) if error.is_custom_emoji_error() => {
                emoji::mark_not_allowed();
                log::warn("[editMessageText] кастомные эмодзи недоступны, правлю с обычными");
                self.edit_once(chat_id, message_id, text, keyboard, false)
            }
            Err(error) => Err(error),
        }
    }

    fn edit_once(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&Value>,
        with_emoji: bool,
    ) -> Result<Value, ApiError> {
        let mut payload = Value::obj();
        payload.set("chat_id", chat_id);
        payload.set("message_id", message_id);
        payload.set(
            "text",
            if with_emoji { text.to_string() } else { emoji::strip_html(text) },
        );
        payload.set("parse_mode", "HTML");
        payload.set(
            "link_preview_options",
            Value::from_pairs(vec![("is_disabled", Value::Bool(true))]),
        );
        if let Some(markup) = keyboard {
            let markup = if with_emoji {
                markup.clone()
            } else {
                crate::keyboard::strip_emojis(markup)
            };
            payload.set("reply_markup", markup);
        }
        self.call("editMessageText", &payload)
    }

    pub fn send_photo(
        &self,
        chat_id: i64,
        photo_path: &std::path::Path,
        caption: &str,
        keyboard: Option<&Value>,
    ) -> Result<Value, ApiError> {
        match self.send_photo_once(chat_id, photo_path, caption, keyboard, true) {
            Ok(value) => Ok(value),
            Err(error) if error.is_custom_emoji_error() => {
                emoji::mark_not_allowed();
                log::warn("[sendPhoto] кастомные эмодзи недоступны, подпись с обычными");
                self.send_photo_once(chat_id, photo_path, caption, keyboard, false)
            }
            Err(error) => Err(error),
        }
    }

    fn send_photo_once(
        &self,
        chat_id: i64,
        photo_path: &std::path::Path,
        caption: &str,
        keyboard: Option<&Value>,
        with_emoji: bool,
    ) -> Result<Value, ApiError> {
        let content = std::fs::read(photo_path).map_err(|error| ApiError {
            code: 0,
            description: format!("не читается файл {}: {}", photo_path.display(), error),
        })?;
        let filename = photo_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "banner.jpg".to_string());

        let mut fields = vec![
            ("chat_id".to_string(), chat_id.to_string()),
            (
                "caption".to_string(),
                if with_emoji {
                    caption.to_string()
                } else {
                    emoji::strip_html(caption)
                },
            ),
            ("parse_mode".to_string(), "HTML".to_string()),
        ];

        if let Some(markup) = keyboard {
            let markup = if with_emoji {
                markup.clone()
            } else {
                crate::keyboard::strip_emojis(markup)
            };
            fields.push(("reply_markup".to_string(), markup.to_string()));
        }

        self.call_multipart(
            "sendPhoto",
            fields,
            Some((
                "photo",
                filename.clone(),
                content,
                crate::http::mime_of(&filename).to_string(),
            )),
        )
    }

    pub fn edit_message_media(
        &self,
        chat_id: i64,
        message_id: i64,
        photo_path: &std::path::Path,
        caption: &str,
        keyboard: Option<&Value>,
    ) -> Result<Value, ApiError> {
        let content = std::fs::read(photo_path).map_err(|error| ApiError {
            code: 0,
            description: format!("не читается файл {}: {}", photo_path.display(), error),
        })?;
        let filename = photo_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "banner.jpg".to_string());

        let media = Value::from_pairs(vec![
            ("type", Value::str("photo")),
            ("media", Value::str("attach://photo")),
            ("caption", Value::str(caption)),
            ("parse_mode", Value::str("HTML")),
        ]);

        let mut fields = vec![
            ("chat_id".to_string(), chat_id.to_string()),
            ("message_id".to_string(), message_id.to_string()),
            ("media".to_string(), media.to_string()),
        ];
        if let Some(markup) = keyboard {
            fields.push(("reply_markup".to_string(), markup.to_string()));
        }

        self.call_multipart(
            "editMessageMedia",
            fields,
            Some((
                "photo",
                filename.clone(),
                content,
                crate::http::mime_of(&filename).to_string(),
            )),
        )
    }

    pub fn send_document(
        &self,
        chat_id: i64,
        filename: &str,
        content: Vec<u8>,
        caption: &str,
        keyboard: Option<&Value>,
    ) -> Result<Value, ApiError> {
        let mut fields = vec![
            ("chat_id".to_string(), chat_id.to_string()),
            ("caption".to_string(), caption.to_string()),
            ("parse_mode".to_string(), "HTML".to_string()),
        ];
        if let Some(markup) = keyboard {
            fields.push(("reply_markup".to_string(), markup.to_string()));
        }
        self.call_multipart(
            "sendDocument",
            fields,
            Some((
                "document",
                filename.to_string(),
                content,
                crate::http::mime_of(filename).to_string(),
            )),
        )
    }

    pub fn answer_callback_query(&self, callback_id: &str, text: Option<&str>, alert: bool) {
        let mut payload = Value::obj();
        payload.set("callback_query_id", callback_id.to_string());
        if let Some(message) = text {
            if !message.is_empty() {
                payload.set("text", message.to_string());
                payload.set("show_alert", alert);
            }
        }
        // «Query is too old» — не критично, поэтому ошибку глотаем
        if let Err(error) = self.call("answerCallbackQuery", &payload) {
            log::debug(format!("answerCallbackQuery: {}", error));
        }
    }

    /// Отклонение платежа с причиной — пользователь увидит её до оплаты.
    pub fn reject_pre_checkout(&self, query_id: &str, reason: &str) {
        let payload = Value::from_pairs(vec![
            ("pre_checkout_query_id", Value::str(query_id)),
            ("ok", Value::Bool(false)),
            ("error_message", Value::str(reason)),
        ]);
        let _ = self.call("answerPreCheckoutQuery", &payload);
    }

    pub fn answer_pre_checkout_query(&self, query_id: &str) {
        let payload = Value::from_pairs(vec![
            ("pre_checkout_query_id", Value::str(query_id)),
            ("ok", Value::Bool(true)),
        ]);
        let _ = self.call("answerPreCheckoutQuery", &payload);
    }

    pub fn delete_message(&self, chat_id: i64, message_id: i64) {
        let payload = Value::from_pairs(vec![
            ("chat_id", Value::Int(chat_id)),
            ("message_id", Value::Int(message_id)),
        ]);
        // сообщение могли уже удалить
        let _ = self.call("deleteMessage", &payload);
    }

    pub fn send_invoice(
        &self,
        chat_id: i64,
        title: &str,
        description: &str,
        invoice_payload: &str,
        amount: i64,
    ) -> Result<Value, ApiError> {
        let prices = Value::arr(vec![Value::from_pairs(vec![
            ("label", Value::str(title)),
            ("amount", Value::Int(amount)),
        ])]);

        let payload = Value::from_pairs(vec![
            ("chat_id", Value::Int(chat_id)),
            ("title", Value::str(title)),
            ("description", Value::str(description)),
            ("payload", Value::str(invoice_payload)),
            ("provider_token", Value::str("")),
            ("currency", Value::str("XTR")),
            ("prices", prices),
        ]);
        self.call("sendInvoice", &payload)
    }

    pub fn answer_inline_query(&self, query_id: &str, results: Vec<Value>) -> Result<Value, ApiError> {
        let payload = Value::from_pairs(vec![
            ("inline_query_id", Value::str(query_id)),
            ("results", Value::arr(results)),
            ("cache_time", Value::Int(5)),
            ("is_personal", Value::Bool(true)),
        ]);
        self.call("answerInlineQuery", &payload)
    }

    pub fn get_file(&self, file_id: &str) -> Result<Value, ApiError> {
        let payload = Value::from_pairs(vec![("file_id", Value::str(file_id))]);
        self.call("getFile", &payload)
    }

    /// Скачивание файла по `file_path` из getFile.
    pub fn download_file(&self, file_path: &str) -> Result<Vec<u8>, ApiError> {
        let api_root = self
            .base
            .rsplit_once("/bot")
            .map(|(prefix, _)| prefix.to_string())
            .unwrap_or_else(|| "https://api.telegram.org".to_string());
        let url = format!("{}/file/bot{}/{}", api_root, self.token, encode_path(file_path));
        let response = self.http.get(&url, 120).map_err(|error| ApiError {
            code: 0,
            description: format!("download: {}", error),
        })?;
        if response.status != 200 {
            return Err(ApiError {
                code: response.status as i64,
                description: format!("download: HTTP {}", response.status),
            });
        }
        Ok(response.body)
    }

    pub fn copy_message(&self, chat_id: i64, from_chat_id: i64, message_id: i64) -> Result<Value, ApiError> {
        let payload = Value::from_pairs(vec![
            ("chat_id", Value::Int(chat_id)),
            ("from_chat_id", Value::Int(from_chat_id)),
            ("message_id", Value::Int(message_id)),
        ]);
        self.call("copyMessage", &payload)
    }
}

/// Простое логирование с уровнями (без крейта log).
pub mod log {
    use std::sync::atomic::{AtomicBool, Ordering};

    static VERBOSE: AtomicBool = AtomicBool::new(false);

    pub fn set_verbose(value: bool) {
        VERBOSE.store(value, Ordering::Relaxed);
    }

    fn timestamp() -> String {
        // Секунды от эпохи заменяем на читаемое время через libc-независимый расчёт.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let days = now / 86400;
        let seconds_of_day = now % 86400;
        let (hours, minutes, seconds) = (
            seconds_of_day / 3600,
            (seconds_of_day % 3600) / 60,
            seconds_of_day % 60,
        );

        // алгоритм перевода дней от 1970-01-01 в дату
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
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year,
            month,
            remaining + 1,
            hours,
            minutes,
            seconds
        )
    }

    pub fn info(message: impl AsRef<str>) {
        println!("{} INFO  {}", timestamp(), message.as_ref());
    }

    pub fn warn(message: impl AsRef<str>) {
        eprintln!("{} WARN  {}", timestamp(), message.as_ref());
    }

    pub fn error(message: impl AsRef<str>) {
        eprintln!("{} ERROR {}", timestamp(), message.as_ref());
    }

    pub fn debug(message: impl AsRef<str>) {
        if VERBOSE.load(Ordering::Relaxed) {
            println!("{} DEBUG {}", timestamp(), message.as_ref());
        }
    }
}
