//! Хранилище данных.
//!
//! Формат: JSON-файл с атомарной записью (временный файл + rename).
//! SQLite сознательно не используется: libsqlite3 в системе есть, но без
//! заголовков и крейта нужен ручной FFI на десятки функций, а объём данных
//! у бота небольшой — файла достаточно.
//!
//! Тексты переписок шифруются AES-256-GCM (см. `crypto`), как и в прежних
//! версиях: формат `enc:v1:nonce:tag:payload` совместим.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::crypto::{gcm_decrypt, gcm_encrypt, sha256};
use crate::json::Value;

const ENC_PREFIX: &str = "enc:v1:";

/// Возвращает текущее время в секундах.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Псевдослучайные байты (nonce для AES-GCM).
///
/// Источник: /dev/urandom. Если он недоступен — смесь времени и адреса
/// стека; этого достаточно для nonce, который никогда не повторяется.
fn random_bytes<const N: usize>() -> [u8; N] {
    use std::io::Read;

    let mut buffer = [0u8; N];

    // /dev/urandom — бесконечный поток, читаем ровно N байт
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        if file.read_exact(&mut buffer).is_ok() {
            return buffer;
        }
    }

    // запасной вариант: время + счётчик
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut state = nanos as u64 ^ (&buffer as *const _ as u64);
    for byte in buffer.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state & 0xff) as u8;
    }
    buffer
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits = 0;

    for ch in text.chars() {
        if ch == '=' || ch.is_whitespace() {
            continue;
        }
        let value = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 26,
            '0'..='9' => ch as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return None,
        };
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// Хранилище на JSON-файле.
pub struct Store {
    path: PathBuf,
    data: Value,
    key: [u8; 32],
    dirty: bool,
}

impl Store {
    pub fn open(path: &Path, encryption_password: &str) -> Self {
        let key = sha256(encryption_password.as_bytes());

        let data = if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(text) => Value::parse(&text).unwrap_or_else(|error| {
                    crate::api::log::error(format!(
                        "База повреждена ({}), начинаю с чистой",
                        error
                    ));
                    Value::obj()
                }),
                Err(error) => {
                    crate::api::log::error(format!("Не читается база: {}", error));
                    Value::obj()
                }
            }
        } else {
            Value::obj()
        };

        let mut store = Store {
            path: path.to_path_buf(),
            data,
            key,
            dirty: false,
        };
        store.ensure_sections();
        store
    }

    fn ensure_sections(&mut self) {
        for section in ["users", "connections", "messages"] {
            if self.data.get(section).is_none() {
                self.data.set(section, Value::obj());
            }
        }
    }

    // ------------------------------------------------------- шифрование

    pub fn encrypt(&self, text: &str) -> String {
        if text.is_empty() || text.starts_with(ENC_PREFIX) {
            return text.to_string();
        }
        let nonce: [u8; 12] = random_bytes();
        let (ciphertext, tag) = gcm_encrypt(&self.key, &nonce, text.as_bytes(), b"");
        format!(
            "{}{}:{}:{}",
            ENC_PREFIX,
            base64_encode(&nonce),
            base64_encode(&tag),
            base64_encode(&ciphertext)
        )
    }

    pub fn decrypt(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        if !text.starts_with(ENC_PREFIX) {
            // старые записи лежали открытым текстом
            return text.to_string();
        }

        let parts: Vec<&str> = text[ENC_PREFIX.len()..].split(':').collect();
        if parts.len() != 3 {
            return String::new();
        }

        let nonce = match base64_decode(parts[0]) {
            Some(bytes) if bytes.len() == 12 => bytes,
            _ => return String::new(),
        };
        let tag = match base64_decode(parts[1]) {
            Some(bytes) if bytes.len() == 16 => bytes,
            _ => return String::new(),
        };
        let ciphertext = match base64_decode(parts[2]) {
            Some(bytes) => bytes,
            None => return String::new(),
        };

        let mut nonce_array = [0u8; 12];
        nonce_array.copy_from_slice(&nonce);
        let mut tag_array = [0u8; 16];
        tag_array.copy_from_slice(&tag);

        match gcm_decrypt(&self.key, &nonce_array, &ciphertext, &tag_array, b"") {
            Some(plain) => String::from_utf8(plain).unwrap_or_default(),
            None => String::new(),
        }
    }

    // ------------------------------------------------------- пользователи

    pub fn user(&self, user_id: i64) -> Option<Value> {
        self.data
            .get("users")
            .and_then(|users| users.get(&user_id.to_string()))
            .cloned()
    }

    /// Создаёт пользователя. Возвращает (запись, создан_ли_сейчас, реферер).
    pub fn create_user(
        &mut self,
        user_id: i64,
        username: &str,
        first_name: &str,
        referred_by: Option<i64>,
    ) -> (Value, bool, Option<i64>) {
        if let Some(existing) = self.user(user_id) {
            return (existing, false, None);
        }

        let mut referrer = None;
        if let Some(candidate) = referred_by {
            if candidate != user_id {
                if let Some(user) = self.user(candidate) {
                    if !user.get_bool("is_banned", false) {
                        referrer = Some(candidate);
                    }
                }
            }
        }

        let timestamp = now();
        let mut user = Value::obj();
        user.set("user_id", user_id);
        user.set("username", username.to_string());
        user.set("first_name", first_name.to_string());
        // приветственные 3 дня
        user.set("subscription_until", timestamp + 3 * 86400);
        user.set("is_premium", false);
        user.set("notify_deleted", true);
        user.set("notify_edited", true);
        user.set("referrals_count", 0i64);
        user.set("is_banned", false);
        user.set("created_at", timestamp);
        if let Some(id) = referrer {
            user.set("referred_by", id);
        }

        self.section_mut("users").set(&user_id.to_string(), user.clone());

        // бонус пригласившему: +7 дней и счётчик
        if let Some(id) = referrer {
            self.add_days(id, 7, false);
            let count = self
                .user(id)
                .map(|user| user.get_i64("referrals_count", 0))
                .unwrap_or(0);
            if let Some(users) = self.data.get_mut("users") {
                if let Some(referrer_entry) = users.entry(&id.to_string()).as_object_mut() {
                    if let Some(value) = referrer_entry.get_mut("referrals_count") {
                        *value = Value::Int(count + 1);
                    }
                }
            }
        }

        self.dirty = true;
        (self.user(user_id).unwrap_or(user), true, referrer)
    }

    fn section_mut(&mut self, name: &str) -> &mut Value {
        if self.data.get(name).is_none() {
            self.data.set(name, Value::obj());
        }
        self.data.entry(name)
    }

    pub fn update_user(&mut self, user_id: i64, field: &str, value: Value) {
        let key = user_id.to_string();
        if let Some(users) = self.data.get_mut("users") {
            if let Some(map) = users.as_object_mut() {
                if let Some(user) = map.get_mut(&key) {
                    user.set(field, value);
                    self.dirty = true;
                }
            }
        }
    }

    pub fn add_days(&mut self, user_id: i64, days: i64, premium: bool) {
        let key = user_id.to_string();
        let current = now();
        if let Some(users) = self.data.get_mut("users") {
            if let Some(map) = users.as_object_mut() {
                if let Some(user) = map.get_mut(&key) {
                    let until = user.get_i64("subscription_until", 0).max(current);
                    user.set("subscription_until", until + days * 86400);
                    if premium {
                        user.set("is_premium", true);
                    }
                    self.dirty = true;
                }
            }
        }
    }

    pub fn set_premium(&mut self, user_id: i64, premium: bool) {
        self.update_user(user_id, "is_premium", Value::Bool(premium));
    }

    pub fn reset_subscription(&mut self, user_id: i64) {
        self.update_user(user_id, "subscription_until", Value::Int(0));
        self.update_user(user_id, "is_premium", Value::Bool(false));
    }

    pub fn set_ban(&mut self, user_id: i64, banned: bool) {
        self.update_user(user_id, "is_banned", Value::Bool(banned));
    }

    pub fn has_premium(&self, user_id: i64, is_admin: bool) -> bool {
        if is_admin {
            return true;
        }
        self.user(user_id)
            .map(|user| user.get_bool("is_premium", false))
            .unwrap_or(false)
    }

    pub fn is_banned(&self, user_id: i64) -> bool {
        self.user(user_id)
            .map(|user| user.get_bool("is_banned", false))
            .unwrap_or(false)
    }

    pub fn all_users(&self) -> Vec<Value> {
        self.data
            .get("users")
            .and_then(|users| users.as_object())
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Статистика для админ-панели.
    pub fn stats(&self) -> (i64, i64, i64, i64, i64) {
        let users = self.all_users();
        let current = now();
        let total = users.len() as i64;
        let active = users
            .iter()
            .filter(|user| {
                user.get_i64("subscription_until", 0) > current && !user.get_bool("is_banned", false)
            })
            .count() as i64;
        let premium = users
            .iter()
            .filter(|user| user.get_bool("is_premium", false) && !user.get_bool("is_banned", false))
            .count() as i64;
        let banned = users
            .iter()
            .filter(|user| user.get_bool("is_banned", false))
            .count() as i64;
        let connections = self
            .data
            .get("connections")
            .and_then(|value| value.as_object())
            .map(|map| {
                map.values()
                    .filter(|connection| connection.get_bool("is_enabled", false))
                    .count()
            })
            .unwrap_or(0) as i64;
        (total, active, premium, banned, connections)
    }

    // -------------------------------------------------------- подключения

    pub fn save_connection(&mut self, owner_id: i64, connection_id: &str, enabled: bool) {
        let mut record = Value::obj();
        record.set("user_id", owner_id);
        record.set("is_enabled", enabled);
        record.set("updated_at", now());
        self.section_mut("connections")
            .set(connection_id, record);
        self.dirty = true;
    }

    pub fn connection_of(&self, owner_id: i64) -> Option<Value> {
        self.data
            .get("connections")
            .and_then(|value| value.as_object())
            .and_then(|map| {
                map.values()
                    .filter(|connection| {
                        connection.get_i64("user_id", 0) == owner_id
                            && connection.get_bool("is_enabled", false)
                    })
                    .cloned()
                    .next()
            })
    }

    pub fn owner_of_connection(&self, connection_id: &str) -> Option<i64> {
        self.data
            .get("connections")
            .and_then(|value| value.get(connection_id))
            .map(|connection| connection.get_i64("user_id", 0))
    }

    // ---------------------------------------------------------- сообщения

    pub fn save_message(&mut self, message: NewMessage) {
        let chat_key = format!("{}:{}", message.owner_id, message.chat_id);
        let message_key = message.message_id.to_string();

        let mut record = Value::obj();
        record.set("owner_id", message.owner_id);
        record.set("chat_id", message.chat_id);
        record.set("message_id", message.message_id);
        record.set("sender_id", message.sender_id.unwrap_or(0));
        record.set("sender_name", self.encrypt(&message.sender_name));
        record.set("sender_username", self.encrypt(&message.sender_username));
        record.set("peer_name", self.encrypt(&message.peer_name));
        record.set("peer_username", self.encrypt(&message.peer_username));
        record.set("text", self.encrypt(&message.text));
        if let Some(kind) = &message.media_type {
            record.set("media_type", kind.clone());
        }
        if let Some(file_id) = &message.file_id {
            record.set("file_id", file_id.clone());
        }
        if let Some(file_name) = &message.file_name {
            record.set("file_name", self.encrypt(file_name));
        }
        record.set("is_outgoing", message.is_outgoing);
        record.set("is_deleted", false);
        record.set("date", message.date);

        if self.data.get("messages").is_none() {
            self.data.set("messages", Value::obj());
        }
        let mut chat = Value::obj();
        if let Some(existing) = self
            .data
            .get("messages")
            .and_then(|messages| messages.get(&chat_key))
            .cloned()
        {
            chat = existing;
        }
        chat.set(&message_key, record);
        self.section_mut("messages").set(&chat_key, chat);
        self.dirty = true;
    }

    pub fn mark_deleted(&mut self, owner_id: i64, chat_id: i64, message_ids: &[i64]) {
        let chat_key = format!("{}:{}", owner_id, chat_id);
        if let Some(messages) = self.data.get_mut("messages") {
            if let Some(chat) = messages.get_mut(&chat_key) {
                if let Some(map) = chat.as_object_mut() {
                    for message_id in message_ids {
                        if let Some(record) = map.get_mut(&message_id.to_string()) {
                            record.set("is_deleted", true);
                            self.dirty = true;
                        }
                    }
                }
            }
        }
    }

    pub fn save_edited_text(&mut self, owner_id: i64, chat_id: i64, message_id: i64, text: &str) {
        let chat_key = format!("{}:{}", owner_id, chat_id);
        let encrypted = self.encrypt(text);
        if let Some(messages) = self.data.get_mut("messages") {
            if let Some(chat) = messages.get_mut(&chat_key) {
                if let Some(map) = chat.as_object_mut() {
                    if let Some(record) = map.get_mut(&message_id.to_string()) {
                        record.set("text", encrypted);
                        self.dirty = true;
                    }
                }
            }
        }
    }

    /// Список чатов пользователя: (chat_id, читаемый заголовок).
    pub fn chats_of(&self, owner_id: i64) -> Vec<(i64, String)> {
        let mut result = Vec::new();
        if let Some(messages) = self.data.get("messages").and_then(|value| value.as_object()) {
            for (chat_key, chat) in messages {
                let mut parts = chat_key.split(':');
                let owner = parts.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                let chat_id = parts.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                if owner != owner_id {
                    continue;
                }

                // имя берём из последнего сообщения с непустым peer_name
                let mut title = String::new();
                if let Some(map) = chat.as_object() {
                    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
                    entries.sort_by_key(|(key, _)| key.parse::<i64>().unwrap_or(0));
                    for (_, record) in entries.iter().rev() {
                        let peer_name = self.decrypt(&record.get_str("peer_name"));
                        let peer_username = self.decrypt(&record.get_str("peer_username"));
                        if !peer_name.is_empty() || !peer_username.is_empty() {
                            title = if peer_name.is_empty() {
                                format!("@{}", peer_username)
                            } else if peer_username.is_empty() {
                                peer_name
                            } else {
                                format!("{} (@{})", peer_name, peer_username)
                            };
                            break;
                        }
                    }
                }
                if title.is_empty() {
                    title = format!("Чат {}", chat_id);
                }
                result.push((chat_id, title));
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Сообщения чата в порядке времени, с расшифрованными текстами.
    pub fn messages_of(&self, owner_id: i64, chat_id: i64) -> Vec<Value> {
        let chat_key = format!("{}:{}", owner_id, chat_id);
        let mut result = Vec::new();

        if let Some(chat) = self
            .data
            .get("messages")
            .and_then(|messages| messages.get(&chat_key))
            .and_then(|chat| chat.as_object())
        {
            for record in chat.values() {
                let mut copy = record.clone();
                let text = self.decrypt(&record.get_str("text"));
                copy.set("text", text);
                let sender = self.decrypt(&record.get_str("sender_name"));
                copy.set("sender_name", sender);
                let file_name = self.decrypt(&record.get_str("file_name"));
                copy.set("file_name", file_name);
                result.push(copy);
            }
        }

        result.sort_by_key(|record| record.get_i64("date", 0));
        result
    }

    // ------------------------------------------------------------- запись

    /// Сохраняет файл, если были изменения.
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        if let Err(error) = self.save() {
            crate::api::log::error(format!("Не удалось сохранить базу: {}", error));
        }
    }

    fn save(&mut self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        // атомарная запись: сначала во временный файл, потом rename
        let temp = self.path.with_extension("tmp");
        std::fs::write(&temp, self.data.to_string())?;
        std::fs::rename(&temp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}

/// Данные для сохранения сообщения.
pub struct NewMessage {
    pub owner_id: i64,
    pub chat_id: i64,
    pub message_id: i64,
    pub sender_id: Option<i64>,
    pub sender_name: String,
    pub sender_username: String,
    pub peer_name: String,
    pub peer_username: String,
    pub text: String,
    pub media_type: Option<String>,
    pub file_id: Option<String>,
    pub file_name: Option<String>,
    pub is_outgoing: bool,
    pub date: i64,
}

/// Читаемое имя собеседника из объекта chat.
pub fn peer_title(chat: &Value) -> String {
    let title = chat.get_str("title");
    if !title.is_empty() {
        return title;
    }
    let name = format!(
        "{} {}",
        chat.get_str("first_name"),
        chat.get_str("last_name")
    )
    .trim()
    .to_string();
    if !name.is_empty() {
        return name;
    }
    let username = chat.get_str("username");
    if username.is_empty() {
        String::new()
    } else {
        format!("@{}", username)
    }
}

/// Карта вида «все пользователи для рассылки».
pub fn broadcast_targets(store: &Store) -> Vec<i64> {
    let mut ids = BTreeMap::new();
    for user in store.all_users() {
        let id = user.get_i64("user_id", 0);
        if id > 0 {
            ids.insert(id, ());
        }
    }
    ids.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str, password: &str) -> Store {
        let path = std::env::temp_dir().join(format!("savemod_test_{}.json", name));
        let _ = std::fs::remove_file(&path);
        Store::open(&path, password)
    }

    #[test]
    fn шифрование_обратимо() {
        let store = temp_store("enc", "пароль-ключ");
        let secret = "секретное сообщение с эмодзи 🔐";
        let encrypted = store.encrypt(secret);
        assert!(encrypted.starts_with("enc:v1:"));
        assert!(!encrypted.contains(secret));
        assert_eq!(store.decrypt(&encrypted), secret);
    }

    #[test]
    fn пустая_строка_остаётся_пустой() {
        let store = temp_store("empty", "k");
        assert_eq!(store.encrypt(""), "");
        assert_eq!(store.decrypt(""), "");
    }

    #[test]
    fn старый_открытый_текст_читается() {
        let store = temp_store("plain", "k");
        assert_eq!(store.decrypt("просто текст"), "просто текст");
    }

    #[test]
    fn чужая_строка_не_расшифровывается() {
        let store = temp_store("foreign", "k");
        assert_eq!(store.decrypt("enc:v1:bad:bad:bad"), "");
    }

    #[test]
    fn другой_ключ_не_расшифрует() {
        let first = temp_store("key1", "пароль-1");
        let second = temp_store("key2", "пароль-2");
        let encrypted = first.encrypt("данные");
        assert_eq!(second.decrypt(&encrypted), "");
    }

    #[test]
    fn base64_туда_обратно() {
        for data in [
            b"".to_vec(),
            b"a".to_vec(),
            b"ab".to_vec(),
            b"abc".to_vec(),
            "кириллица".as_bytes().to_vec(),
        ] {
            let encoded = base64_encode(&data);
            assert_eq!(base64_decode(&encoded).unwrap(), data, "сломалось на {:?}", data);
        }
    }

    #[test]
    fn base64_эталон() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }

    #[test]
    fn создание_пользователя_даёт_три_дня() {
        let mut store = temp_store("create", "k");
        let (user, created, referrer) = store.create_user(100, "test", "Тест", None);
        assert!(created);
        assert!(referrer.is_none());
        let until = user.get_i64("subscription_until", 0);
        assert!(until > now() + 2 * 86400, "должно быть ~3 дня");
    }

    #[test]
    fn повторное_создание_не_дублирует() {
        let mut store = temp_store("twice", "k");
        store.create_user(100, "test", "Тест", None);
        let (_, created, _) = store.create_user(100, "test", "Тест", None);
        assert!(!created);
        assert_eq!(store.all_users().len(), 1);
    }

    #[test]
    fn реферал_начисляет_семь_дней() {
        let mut store = temp_store("ref", "k");
        let (referrer, _, _) = store.create_user(100, "a", "A", None);
        let before = referrer.get_i64("subscription_until", 0);

        store.create_user(200, "b", "B", Some(100));

        let after = store.user(100).unwrap();
        assert!(after.get_i64("subscription_until", 0) >= before + 7 * 86400);
        assert_eq!(after.get_i64("referrals_count", 0), 1);
    }

    #[test]
    fn реферал_сам_на_себя_игнорируется() {
        let mut store = temp_store("selfref", "k");
        let (_, _, referrer) = store.create_user(100, "a", "A", Some(100));
        assert!(referrer.is_none());
    }

    #[test]
    fn забаненный_не_приносит_бонус() {
        let mut store = temp_store("bannedref", "k");
        store.create_user(100, "a", "A", None);
        store.set_ban(100, true);
        let (_, _, referrer) = store.create_user(200, "b", "B", Some(100));
        assert!(referrer.is_none());
    }

    #[test]
    fn подписка_продлевается() {
        let mut store = temp_store("extend", "k");
        let (user, _, _) = store.create_user(100, "a", "A", None);
        let before = user.get_i64("subscription_until", 0);
        store.add_days(100, 30, false);
        let after = store.user(100).unwrap().get_i64("subscription_until", 0);
        assert_eq!(after, before + 30 * 86400);
    }

    #[test]
    fn премиум_флаг() {
        let mut store = temp_store("prem", "k");
        store.create_user(100, "a", "A", None);
        assert!(!store.has_premium(100, false));
        store.set_premium(100, true);
        assert!(store.has_premium(100, false));
        // админ всегда с премиумом
        assert!(store.has_premium(999, true));
    }

    #[test]
    fn сообщения_сохраняются_и_расшифровываются() {
        let mut store = temp_store("msg", "k");
        store.save_message(NewMessage {
            owner_id: 1,
            chat_id: 42,
            message_id: 7,
            sender_id: Some(42),
            sender_name: "Вася".to_string(),
            sender_username: "vasya".to_string(),
            peer_name: "Вася".to_string(),
            peer_username: "vasya".to_string(),
            text: "привет".to_string(),
            media_type: None,
            file_id: None,
            file_name: None,
            is_outgoing: false,
            date: 1700000000,
        });

        let messages = store.messages_of(1, 42);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].get_str("text"), "привет");
        assert_eq!(messages[0].get_str("sender_name"), "Вася");
    }

    #[test]
    fn список_чатов_с_именем() {
        let mut store = temp_store("chats", "k");
        store.save_message(NewMessage {
            owner_id: 1,
            chat_id: 42,
            message_id: 1,
            sender_id: Some(42),
            sender_name: "Вася".to_string(),
            sender_username: "vasya".to_string(),
            peer_name: "Вася".to_string(),
            peer_username: "vasya".to_string(),
            text: "привет".to_string(),
            media_type: None,
            file_id: None,
            file_name: None,
            is_outgoing: false,
            date: 1700000000,
        });
        let chats = store.chats_of(1);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].0, 42);
        assert!(chats[0].1.contains("Вася"), "заголовок: {}", chats[0].1);
    }

    #[test]
    fn удаление_помечает_сообщение() {
        let mut store = temp_store("del", "k");
        store.save_message(NewMessage {
            owner_id: 1,
            chat_id: 42,
            message_id: 7,
            sender_id: None,
            sender_name: String::new(),
            sender_username: String::new(),
            peer_name: String::new(),
            peer_username: String::new(),
            text: "удалится".to_string(),
            media_type: None,
            file_id: None,
            file_name: None,
            is_outgoing: false,
            date: 1700000000,
        });
        store.mark_deleted(1, 42, &[7]);
        let messages = store.messages_of(1, 42);
        assert!(messages[0].get_bool("is_deleted", false));
    }

    #[test]
    fn сохранение_на_диск_и_чтение() {
        let path = std::env::temp_dir().join("savemod_persist.json");
        let _ = std::fs::remove_file(&path);
        {
            let mut store = Store::open(&path, "k");
            store.create_user(777, "persist", "Тест", None);
            store.flush();
        }
        let store = Store::open(&path, "k");
        assert!(store.user(777).is_some());
        assert_eq!(store.user(777).unwrap().get_str("username"), "persist");
    }

    #[test]
    fn статистика_считается() {
        let mut store = temp_store("stats", "k");
        store.create_user(1, "a", "A", None);
        store.create_user(2, "b", "B", None);
        store.set_ban(2, true);
        let (total, active, _, banned, _) = store.stats();
        assert_eq!(total, 2);
        assert_eq!(active, 1);
        assert_eq!(banned, 1);
    }

    #[test]
    fn подключение_сохраняется() {
        let mut store = temp_store("conn", "k");
        store.save_connection(100, "conn_1", true);
        assert!(store.connection_of(100).is_some());
        assert_eq!(store.owner_of_connection("conn_1"), Some(100));
        store.save_connection(100, "conn_1", false);
        assert!(store.connection_of(100).is_none());
    }
}
