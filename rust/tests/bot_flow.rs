//! Интеграционные тесты: фейковый Telegram + настоящие обработчики.
//!
//! Проверяем именно те сценарии, что ломались в проде:
//!   1. меню с кастомными эмодзи падает → бот повторяет без них (DOCUMENT_INVALID);
//!   2. нажатие кнопки на фото-меню → «there is no text to edit» → новое сообщение;
//!   3. баннеры отправляются с правильными именами файлов;
//!   4. бизнес-события (удаление/правка) и команды управления работают;
//!   5. «Создать бота» больше нет, а callback_data не совпадает с текстом.

mod fake_telegram;

use fake_telegram::FakeTelegram;
use savemod_bot::api::Api;
use savemod_bot::config::Config;
use savemod_bot::handlers::Handlers;
use savemod_bot::json::Value;
use savemod_bot::sender::Sender;
use savemod_bot::store::Store;

fn test_config(base_url: &str, database: std::path::PathBuf) -> Config {
    Config {
        bot_token: "123456:TESTTOKEN".to_string(),
        admin_ids: vec![1],
        database_path: database,
        encryption_key: "test-key".to_string(),
        api_base: base_url.to_string(),
        assets_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets"),
        max_export_mb: 48,
        max_single_media_mb: 35,
        max_import_file_mb: 200,
        max_imported_media_mb: 100,
        custom_emoji: true,
        verbose: false,
        temp_dir: std::env::temp_dir(),
    }
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("savemod_it_{}.json", name));
    let _ = std::fs::remove_file(&path);
    path
}

fn user_update(user_id: i64, text: &str) -> Value {
    Value::from_pairs(vec![
        ("update_id", Value::Int(1)),
        (
            "message",
            Value::from_pairs(vec![
                ("message_id", Value::Int(10)),
                (
                    "from",
                    Value::from_pairs(vec![
                        ("id", Value::Int(user_id)),
                        ("first_name", Value::str("Тест")),
                        ("username", Value::str("tester")),
                    ]),
                ),
                (
                    "chat",
                    Value::from_pairs(vec![
                        ("id", Value::Int(user_id)),
                        ("type", Value::str("private")),
                    ]),
                ),
                ("text", Value::str(text)),
                ("date", Value::Int(1700000000)),
            ]),
        ),
    ])
}

fn callback_update(user_id: i64, data: &str, message_id: i64) -> Value {
    Value::from_pairs(vec![
        ("update_id", Value::Int(2)),
        (
            "callback_query",
            Value::from_pairs(vec![
                ("id", Value::str("cb1")),
                ("data", Value::str(data)),
                (
                    "from",
                    Value::from_pairs(vec![
                        ("id", Value::Int(user_id)),
                        ("first_name", Value::str("Тест")),
                    ]),
                ),
                (
                    "message",
                    Value::from_pairs(vec![
                        ("message_id", Value::Int(message_id)),
                        (
                            "chat",
                            Value::from_pairs(vec![("id", Value::Int(user_id))]),
                        ),
                    ]),
                ),
            ]),
        ),
    ])
}

#[test]
fn старт_показывает_меню_с_баннером() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("start"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&user_update(100, "/start"));

    let photos = fake.calls_of("sendPhoto");
    assert_eq!(photos.len(), 1, "меню должно уйти с баннером: {:?}", fake.timeline());
    // имя файла приходит полем multipart — именно mainmenu.jpg из конфига
    assert_eq!(
        photos[0].payload.get_str("photo"),
        "mainmenu.jpg",
        "отправлен не тот баннер: {:?}",
        photos[0].payload
    );
    assert!(fake.calls_of("sendMessage").is_empty(), "лишнее сообщение без фото");
}

#[test]
fn команда_не_удаляет_сообщение_пользователя() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("no_delete"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&user_update(100, "/start"));

    let deleted = fake.calls_of("deleteMessage");
    assert!(
        deleted.is_empty(),
        "бот удалил сообщение пользователя: {:?}",
        fake.timeline()
    );
}

#[test]
fn при_отказе_эмодзи_меню_уходит_без_них() {
    let fake = FakeTelegram::start();
    fake.reject_custom_emoji(true);

    let config = test_config(&fake.base_url(), temp_db("emoji"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&user_update(100, "/start"));

    // первый заход получил отказ, второй прошёл без эмодзи
    let errors = fake.errors();
    assert!(!errors.is_empty(), "фейк не отказал: {:?}", fake.timeline());

    let delivered = fake
        .all_calls()
        .into_iter()
        .filter(|call| call.method == "sendPhoto" || call.method == "sendMessage")
        .filter(|call| !call.has_emoji_tags() && !call.any_text().trim().is_empty())
        .count();
    assert!(delivered > 0, "меню не дошло после отказа: {:?}", fake.timeline());

    // и текст всё-таки есть
    let texts: Vec<String> = fake
        .all_calls()
        .iter()
        .map(|call| call.any_text())
        .filter(|text| text.contains("Привет"))
        .collect();
    assert!(!texts.is_empty(), "нет приветствия: {:?}", fake.timeline());
}

#[test]
fn нередактируемое_фото_вызывает_новое_сообщение() {
    let fake = FakeTelegram::start();
    fake.mark_no_text(55);

    let config = test_config(&fake.base_url(), temp_db("edit"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    // пользователь жмёт кнопку под старым фото-меню (message_id 55);
    // экран «проверка подключения» — текстовый, значит идёт через edit
    handlers.handle(&callback_update(100, "check_connection", 55));

    let errors = fake.errors();
    assert!(
        errors.iter().any(|(_, description)| description.contains("no text in the message")),
        "фейк не сымитировал отказ: {:?}",
        fake.timeline()
    );

    // бот не сдался: отправил новое сообщение/фото
    let delivered = fake.calls_of("sendMessage").len() + fake.calls_of("sendPhoto").len();
    assert!(delivered > 0, "после отказа ничего не отправлено: {:?}", fake.timeline());

    // и ответ по существу показан
    let has_answer = fake
        .all_calls()
        .iter()
        .any(|call| call.any_text().contains("Подключение"));
    assert!(has_answer, "ответ не показан: {:?}", fake.timeline());
}

#[test]
fn кнопка_тарифов_показывает_три_тарифа() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("tariffs"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&callback_update(100, "tariffs", 0));

    let keyboard = fake
        .all_calls()
        .last()
        .and_then(|call| call.keyboard())
        .expect("нет клавиатуры");
    let serialized = keyboard.to_string();
    assert!(serialized.contains("buy_sub_7"), "нет тарифа 7: {}", serialized);
    assert!(serialized.contains("buy_sub_30"), "нет тарифа 30");
    assert!(serialized.contains("buy_sub_90"), "нет тарифа 90");
    assert!(serialized.contains("buy_sub_premium"), "нет премиума");
    assert!(!serialized.contains("create_bot"), "кнопка «Создать бота» вернулась!");
}

#[test]
fn покупка_уходит_инвойсом_в_звёздах() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("invoice"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&callback_update(100, "buy_sub_30", 0));

    let invoices = fake.calls_of("sendInvoice");
    assert_eq!(invoices.len(), 1, "инвойс не отправлен: {:?}", fake.timeline());

    let payload = &invoices[0].payload;
    assert_eq!(payload.get_str("currency"), "XTR", "валюта не звёзды");
    assert!(payload.get_str("payload").contains("sub_30"));
}

#[test]
fn бизнес_удаление_помечает_сообщения_и_уведомляет() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("biz_del"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    // владелец подключил бота
    handlers.handle(&Value::from_pairs(vec![
        ("update_id", Value::Int(3)),
        (
            "business_connection",
            Value::from_pairs(vec![
                ("id", Value::str("conn_1")),
                ("is_enabled", Value::Bool(true)),
                (
                    "user",
                    Value::from_pairs(vec![("id", Value::Int(1))]),
                ),
            ]),
        ),
    ]));

    // пришло сообщение от собеседника
    handlers.handle(&business_message("conn_1", 500, 7, "привет"));

    // собеседник удалил его
    handlers.handle(&Value::from_pairs(vec![
        ("update_id", Value::Int(4)),
        (
            "deleted_business_messages",
            Value::from_pairs(vec![
                ("business_connection_id", Value::str("conn_1")),
                (
                    "chat",
                    Value::from_pairs(vec![("id", Value::Int(500))]),
                ),
                ("message_ids", Value::Array(vec![Value::Int(7)])),
            ]),
        ),
    ]));

    handlers.flush();

    let messages = handlers.store.messages_of(1, 500);
    assert_eq!(messages.len(), 1, "сообщение не сохранено");
    assert!(messages[0].get_bool("is_deleted", false), "не помечено удалённым");

    let notices: Vec<String> = fake
        .calls_of("sendMessage")
        .iter()
        .map(|call| call.text())
        .collect();
    assert!(
        notices.iter().any(|text| text.contains("удален")),
        "владельцу не сообщили об удалении: {:?}",
        notices
    );
}

#[test]
fn бизнес_команды_управляют_перехватом() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("biz_cmd"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&Value::from_pairs(vec![
        ("update_id", Value::Int(3)),
        (
            "business_connection",
            Value::from_pairs(vec![
                ("id", Value::str("conn_1")),
                ("is_enabled", Value::Bool(true)),
                ("user", Value::from_pairs(vec![("id", Value::Int(1))])),
            ]),
        ),
    ]));

    // владелец пишет команду в business-чате: from = владелец, chat = собеседник
    handlers.handle(&owner_business_message("conn_1", 1, 500, 8, ".nah"));
    assert!(
        !handlers.store.user(1).unwrap().get_bool("intercept", true),
        ".nah не выключил перехват"
    );

    handlers.handle(&owner_business_message("conn_1", 1, 500, 9, ".love"));
    assert!(
        handlers.store.user(1).unwrap().get_bool("intercept", false),
        ".love не включил перехват"
    );
}

#[test]
fn реферальная_ссылка_ведёт_на_бота() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("ref"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    handlers.handle(&callback_update(100, "invite", 0));

    let texts: Vec<String> = fake
        .all_calls()
        .iter()
        .map(|call| call.any_text())
        .collect();
    assert!(
        texts.iter().any(|text| text.contains("start=ref_100")),
        "нет реферальной ссылки: {:?}",
        texts
    );
}

#[test]
fn админ_видит_панель_а_обычный_нет() {
    let fake = FakeTelegram::start();
    let config = test_config(&fake.base_url(), temp_db("admin"));
    let api = Api::new(&config.bot_token, &config.api_base);
    let sender = Sender::new(&config);
    let mut store = Store::open(&config.database_path, &config.encryption_key);
    let mut handlers = Handlers::new(&api, &mut store, &config, &sender, "test_bot");

    // админ (id 1 из конфига)
    handlers.handle(&callback_update(1, "admin", 0));
    let admin_texts: Vec<String> = fake
        .all_calls()
        .iter()
        .map(|call| call.any_text())
        .collect();
    assert!(
        admin_texts.iter().any(|text| text.contains("Админ-панель")),
        "админ не увидел панель: {:?}",
        admin_texts
    );

    fake.reset();

    // обычный пользователь
    handlers.handle(&callback_update(999, "admin", 0));
    let user_texts: Vec<String> = fake
        .all_calls()
        .iter()
        .map(|call| call.any_text())
        .collect();
    assert!(
        !user_texts.iter().any(|text| text.contains("Админ-панель")),
        "обычный пользователь попал в админку: {:?}",
        user_texts
    );
}

/// Сообщение, отправленное владельцем аккаунта в Business-чате.
fn owner_business_message(
    connection: &str,
    owner_id: i64,
    chat_id: i64,
    message_id: i64,
    text: &str,
) -> Value {
    Value::from_pairs(vec![
        ("update_id", Value::Int(10)),
        (
            "business_message",
            Value::from_pairs(vec![
                ("business_connection_id", Value::str(connection)),
                ("message_id", Value::Int(message_id)),
                (
                    "from",
                    Value::from_pairs(vec![
                        ("id", Value::Int(owner_id)),
                        ("first_name", Value::str("Владелец")),
                    ]),
                ),
                (
                    "chat",
                    Value::from_pairs(vec![
                        ("id", Value::Int(chat_id)),
                        ("first_name", Value::str("Собеседник")),
                    ]),
                ),
                ("text", Value::str(text)),
                ("date", Value::Int(1700000200)),
            ]),
        ),
    ])
}

fn business_message(connection: &str, chat_id: i64, message_id: i64, text: &str) -> Value {
    Value::from_pairs(vec![
        ("update_id", Value::Int(9)),
        (
            "business_message",
            Value::from_pairs(vec![
                ("business_connection_id", Value::str(connection)),
                ("message_id", Value::Int(message_id)),
                (
                    "from",
                    Value::from_pairs(vec![
                        ("id", Value::Int(chat_id)),
                        ("first_name", Value::str("Собеседник")),
                        ("username", Value::str("peer")),
                    ]),
                ),
                (
                    "chat",
                    Value::from_pairs(vec![
                        ("id", Value::Int(chat_id)),
                        ("first_name", Value::str("Собеседник")),
                    ]),
                ),
                ("text", Value::str(text)),
                ("date", Value::Int(1700000100)),
            ]),
        ),
    ])
}
