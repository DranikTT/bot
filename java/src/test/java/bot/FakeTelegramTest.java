package bot;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Оффлайн-проверки бота на локальном двойнике Bot API.
 *
 * Запуск: java -cp build/classes:build/test-classes:lib/* bot.FakeTelegramTest
 */
public final class FakeTelegramTest {

    private static int passed;
    private static int failed;

    public static void main(String[] args) throws Exception {
        Path tempDb = Files.createTempDirectory("bottest").resolve("test.sqlite");

        FakeTelegram fake = new FakeTelegram(0);
        fake.start();

        Database db = new Database(tempDb, "test-secret-key");
        TelegramApi api = new TelegramApi("test:token", fake.baseUrl());

        Config config = Config.load();
        MenuSender sender = new MenuSender(api, config);
        UpdateHandler handler = new UpdateHandler(api, db, config, sender);
        handler.setBotUsername("fake_bot");

        check("getMe", () -> {
            Map<String, Object> me = api.getMe();
            assertEquals("fake_bot", me.get("username"));
        });

        // --- главное меню ---
        handler.handle(update("message", message(5001, 1, "/start")));

        check("Старт: отправлено меню", () ->
                assertTrue(apiCalls(fake, "sendMessage") + apiCalls(fake, "sendPhoto") > 0,
                        "меню не отправлено"));

        check("Старт: баннер ушёл фото", () ->
                assertTrue(apiCalls(fake, "sendPhoto") > 0, "нет sendPhoto"));

        check("Старт: никто не ошибся", () ->
                assertEquals(0, fake.errors().size()));

        // --- кастомные эмодзи в HTML ---
        List<Map<String, Object>> photoCalls = fake.callsOf("sendPhoto");
        String menuText = photoCalls.isEmpty() ? "" : Json.getString(photoCalls.get(0), "caption", "");

        check("HTML: есть <tg-emoji>", () ->
                assertTrue(menuText.contains("<tg-emoji emoji-id=\""),
                        "в меню нет тега tg-emoji"));

        check("HTML: есть fallback-эмодзи", () ->
                assertTrue(menuText.contains("</tg-emoji>"), "нет закрывающего тега"));

        check("HTML: parse_mode=HTML", () ->
                assertEquals("HTML", Json.getString(
                        fake.callsOf("sendPhoto").get(0), "parse_mode", "")));

        // --- кнопки с иконками ---
        check("Кнопки: 5 пунктов меню", () -> {
            Object markup = fake.callsOf("sendPhoto").get(0).get("reply_markup");
            String serialized = Json.write(markup);
            for (String data : List.of("tariffs", "invite", "connect", "create_bot", "settings")) {
                assertTrue(serialized.contains(data), "нет кнопки " + data);
            }
        });

        check("Кнопки: кастомные иконки в кнопках", () -> {
            Object markup = fake.callsOf("sendPhoto").get(0).get("reply_markup");
            assertTrue(Json.write(markup).contains("icon_custom_emoji_id"),
                    "нет icon_custom_emoji_id");
        });

        check("Кнопки: callback_data ≠ текст", () -> {
            Object markup = fake.callsOf("sendPhoto").get(0).get("reply_markup");
            String serialized = Json.write(markup);
            if (markup instanceof String) {
                // multipart-поле приходит строкой — разворачиваем
                serialized = Json.getString(
                        Map.of("v", markup), "v", String.valueOf(markup));
            }
            assertTrue(serialized.contains("\"callback_data\""),
                    "callback_data потерялся");
            assertFalse(serialized.contains("\"callback_data\":\"Тарифы\""),
                    "callback_data подменён текстом");
        });

        // --- клики по меню ---
        String[][] routes = {
                {"tariffs", "Тарифы"},
                {"invite", "Приглашение"},
                {"connect", "Подключение"},
                {"create_bot", "Создание бота"},
                {"settings", "Настройки"},
                {"back_main", "Главное"},
        };

        for (String[] route : routes) {
            String data = route[0];
            int before = fake.totalCalls();
            handler.handle(update("callback_query", callback(6001, data, 777)));
            check("Клик «" + route[1] + "»", () ->
                    assertTrue(fake.totalCalls() > before, "нет реакции на " + data));
        }

        // --- тумблеры настроек ---
        handler.handle(update("callback_query", callback(6002, "toggle_deleted", 777)));
        Map<String, Object> user = db.getUser(777);
        check("Тумблер удалённых", () ->
                assertEquals(0L, Json.getLong(user, "notify_deleted", 1)));

        handler.handle(update("callback_query", callback(6003, "toggle_deleted", 777)));
        check("Тумблер удалённых: возврат", () ->
                assertEquals(1L, Json.getLong(db.getUser(777), "notify_deleted", 1)));

        // --- подписка и реферал ---
        handler.handle(update("callback_query", callback(6004, "buy_sub_30", 777)));
        check("Оплата: счёт отправлен", () -> {
            List<Map<String, Object>> invoices = fake.callsOf("sendInvoice");
            assertTrue(!invoices.isEmpty(), "нет счёта");
            assertEquals("XTR", invoices.get(invoices.size() - 1).get("currency"));
        });

        handler.handle(update("message", payment(5002, 777, "sub_30_777")));
        check("Оплата: подписка начислена", () -> {
            Map<String, Object> after = db.getUser(777);
            assertTrue(Json.getLong(after, "subscription_until", 0) > 0,
                    "подписка не начислена");
        });

        // --- реферальная ссылка ---
        handler.handle(update("message", message(5003, 888, "/start ref_777")));
        check("Реферал: пригласивший получил бонус", () -> {
            Map<String, Object> referrer = db.getUser(777);
            assertTrue(Json.getLong(referrer, "referrals_count", 0) >= 1,
                    "не учтён реферал");
        });

        // --- админка ---
        // 777 в ADMIN_IDS — админка открыта; 555 — обычный пользователь
        handler.handle(update("message", message(5004, 777, "/admin")));
        check("Админка: админу открыта", () ->
                assertTrue(lastText(fake).contains("Админ-панель"),
                        "админка не открылась: " + lastText(fake)));

        handler.handle(update("message", message(5006, 555, "/admin")));
        check("Админка: отказ неадмину", () ->
                assertTrue(lastText(fake).contains("нет прав"),
                        "админка открыта постороннему: " + lastText(fake)));

        // --- шифрование ---
        check("Шифрование: обратимо", () -> {
            String secret = "секретное сообщение с эмодзи 🔐";
            String encrypted = db.encrypt(secret);
            assertTrue(encrypted.startsWith("enc:v1:"), "нет префикса enc:v1:");
            assertFalse(encrypted.contains(secret), "текст не зашифрован");
            assertEquals(secret, db.decrypt(encrypted));
        });

        check("Шифрование: пустая строка", () -> assertEquals("", db.encrypt("")));
        check("Шифрование: чужая строка не расшифровывается", () ->
                assertEquals("", db.decrypt("enc:v1:bad:bad:bad")));

        // --- архив ---
        check("Архив: экспорт HTML", () -> {
            db.saveMessage("42", 1, 777, 42L, "Вася", "vasya", "Вася", "vasya",
                    "привет", null, null, null, false, 1700000000);
            Archive.ExportResult result = Archive.export(db, 777, "42", "Вася",
                    Keyboards.ARCHIVE_TYPES, 100 * 1024 * 1024);
            assertTrue(result.count() >= 1, "нет сообщений в экспорте");
            assertTrue(result.html().contains("<!DOCTYPE html>"), "нет HTML-шапки");
            assertTrue(result.html().contains("привет"), "нет текста сообщения");
        });

        check("Архив: XSS экранируется", () -> {
            db.saveMessage("43", 2, 777, 42L, "Вася", "vasya", "Вася", "vasya",
                    "<script>alert(1)</script>", null, null, null, false, 1700000001);
            Archive.ExportResult result = Archive.export(db, 777, "43", "Вася",
                    Keyboards.ARCHIVE_TYPES, 100 * 1024 * 1024);
            assertFalse(result.html().contains("<script>alert(1)</script>"),
                    "XSS не экранирован");
            assertTrue(result.html().contains("&lt;script&gt;"), "нет экранирования");
        });

        check("Архив: фильтр по типам", () -> {
            db.saveMessage("44", 3, 777, 42L, "Вася", "vasya", "Вася", "vasya",
                    "текст", null, null, null, false, 1700000002);
            Archive.ExportResult result = Archive.export(db, 777, "44", "Вася",
                    List.of("photo"), 100 * 1024 * 1024);
            assertEquals(0, result.count());
        });

        // --- импорт ---
        check("Импорт: result.json", () -> {
            Path json = Files.createTempFile("result", ".json");
            String content = "{\"id\": 900, \"name\": \"Тест\", \"messages\": ["
                    + "{\"id\": 1, \"type\": \"message\", \"date\": \"2024-01-01T10:00:00\","
                    + " \"date_unixtime\": 1704103200, \"from\": \"Вася\", \"from_id\": \"user42\","
                    + " \"text\": \"первое сообщение\"},"
                    + "{\"id\": 2, \"type\": \"message\", \"date\": \"2024-01-01T10:01:00\","
                    + " \"date_unixtime\": 1704103260, \"from\": \"Петя\", \"from_id\": \"user43\","
                    + " \"text\": [{\"type\": \"bold\", \"text\": \"второе\"}, \" сообщение\"],"
                    + " \"photo\": \"photos/photo_1.jpg\"}"
                    + "]}";
            Files.writeString(json, content);
            int imported = Archive.importDesktop(json, db, 777);
            assertEquals(2, imported);
            List<Map<String, Object>> messages = db.getChatMessages("900", 777);
            assertEquals(2, messages.size());
            assertTrue(Json.getString(messages.get(0), "text", "").contains("первое"));
        });

        check("Импорт: мусорный JSON отклонён", () -> {
            Path json = Files.createTempFile("bad", ".json");
            Files.writeString(json, "{\"foo\": 1}");
            try {
                Archive.importDesktop(json, db, 777);
                throw new AssertionError("ожидалась ошибка");
            } catch (IllegalArgumentException expected) {
                assertTrue(expected.getMessage().contains("messages"));
            }
        });

        // --- zip-slip ---
        check("Импорт: zip-slip заблокирован", () -> {
            Path zip = Files.createTempFile("evil", ".zip");
            try (var out = new java.util.zip.ZipOutputStream(Files.newOutputStream(zip))) {
                out.putNextEntry(new java.util.zip.ZipEntry("../../evil.txt"));
                out.write("pwned".getBytes());
                out.closeEntry();
                out.putNextEntry(new java.util.zip.ZipEntry("result.json"));
                out.write("{\"messages\": []}".getBytes());
                out.closeEntry();
            }
            Path extracted = Archive.extractZip(zip, Files.createTempDirectory("extract"));
            assertEquals("result.json", extracted.getFileName().toString());
            assertFalse(Files.exists(Path.of("../../evil.txt").toAbsolutePath().normalize()),
                    "zip-slip сработал");
        });

        // --- бан ---
        check("Бан: забаненный получает отказ", () -> {
            db.setBan(888, true);
            int before = fake.callsOf("sendMessage").size();
            handler.handle(update("message", message(5005, 888, "/start")));
            List<Map<String, Object>> messages = fake.callsOf("sendMessage");
            assertTrue(messages.size() > before, "нет ответа забаненному");
            assertTrue(Json.getString(messages.get(messages.size() - 1), "text", "")
                    .contains("заблокирован"), "нет текста о блокировке");
        });

        // --- клавиатура ---
        check("Клавиатура: callback_data не равен тексту", () -> {
            try {
                Keyboard.create().add(new LinkedHashMap<>(Map.of(
                        "text", "Проверка", "callback_data", "Проверка"))).build();
                throw new AssertionError("ожидалась ошибка");
            } catch (IllegalArgumentException expected) {
                assertTrue(expected.getMessage().contains("совпадает"));
            }
        });

        check("Клавиатура: длинный callback_data отклонён", () -> {
            try {
                Keyboard.create().add(new LinkedHashMap<>(Map.of(
                        "text", "X", "callback_data", "a".repeat(70)))).build();
                throw new AssertionError("ожидалась ошибка");
            } catch (IllegalArgumentException expected) {
                assertTrue(expected.getMessage().contains("неверной длины"));
            }
        });

        check("Клавиатура: stripEmojis убирает иконки", () -> {
            Map<String, Object> markup = Keyboards.main(1, config);
            String stripped = Json.write(Keyboard.stripEmojis(markup));
            assertFalse(stripped.contains("icon_custom_emoji_id"), "иконки остались");
            assertTrue(stripped.contains("tariffs"), "кнопки пропали");
        });

        // --- JSON ---
        check("JSON: разбор вложенных структур", () -> {
            Map<String, Object> parsed = Json.parseObject(
                    "{\"a\":[1,2,{\"b\":\"x\\u00e9\"}],\"c\":null,\"d\":true}");
            assertEquals(3, Json.getArray(parsed, "a").size());
            assertEquals("xé", ((Map<?, ?>) Json.getArray(parsed, "a").get(2)).get("b"));
            assertEquals(Boolean.TRUE, parsed.get("d"));
        });

        check("JSON: экранирование при сериализации", () -> {
            String written = Json.write(Map.of("t", "кавычка \" и слэш \\ и перенос \n"));
            assertTrue(written.contains("\\\""), "кавычка не экранирована");
            assertTrue(written.contains("\\n"), "перенос не экранирован");
            assertEquals("кавычка \" и слэш \\ и перенос \n",
                    Json.parseObject(written).get("t"));
        });

        // --- кастомные эмодзи: сверка с оригиналом ---
        check("Эмодзи: все ID валидны", () -> {
            for (Map.Entry<String, String> entry : Emoji.IDS.entrySet()) {
                assertTrue(Emoji.isValidId(entry.getValue()),
                        "некорректный ID у «" + entry.getKey() + "»: " + entry.getValue());
            }
            assertTrue(Emoji.IDS.size() >= 14, "слишком мало эмодзи: " + Emoji.IDS.size());
        });

        check("Эмодзи: ID совпадают с прежней версией", () -> {
            // значения из git show 4de3df5:menuKeys.js и текстов menus.js
            String[][] pairs = {
                    {"wave", "5994750571041525522"},
                    {"back", "5877468380125990242"},
                    {"check", "5776375003280838798"},
                    {"cross", "5778527486270770928"},
                    {"gear", "5877260593903175642"},
                    {"plug", "5877465816030515018"},
                    {"folder", "5877482652302315100"},
                    {"user", "5877477244938489129"},
                    {"gem", "6028530359975548369"},
                    {"star", "5879785854284599288"},
                    {"crown", "5807868868886009920"},
                    {"arrow", "5875008416132370818"},
                    {"shield", "5778423822940114949"},
                    {"clip", "5877495434124988415"},
            };
            Map<String, String> expected = new LinkedHashMap<>();
            for (String[] pair : pairs) {
                expected.put(pair[0], pair[1]);
            }
            for (Map.Entry<String, String> entry : expected.entrySet()) {
                assertEquals(entry.getValue(), Emoji.IDS.get(entry.getKey()));
            }
        });

        check("Эмодзи: html() формирует tg-emoji", () -> {
            String value = Emoji.html("gem", "💎");
            assertEquals("<tg-emoji emoji-id=\"6028530359975548369\">💎</tg-emoji>", value);
        });

        check("Эмодзи: неизвестное имя -> fallback", () ->
                assertEquals("🎯", Emoji.html("unknown", "🎯")));

        check("Эмодзи: кнопка содержит иконку и стиль", () -> {
            Map<String, Object> button = Emoji.button("Тест", "test_cb", "gem", "primary");
            assertEquals("6028530359975548369", button.get("icon_custom_emoji_id"));
            assertEquals("primary", button.get("style"));
            assertEquals("test_cb", button.get("callback_data"));
        });

        // --- поведение при отсутствии права на кастомные эмодзи ---
        // Ровно то, что было в проде: Telegram отвечает DOCUMENT_INVALID
        // на текст с <tg-emoji>, меню должно уйти с обычными эмодзи.

        Emoji.resetForTests();
        fake.rejectCustomEmoji(true);
        fake.reset();

        handler.handle(update("message", message(5101, 9001, "/start")));

        check("Без права: меню всё равно доставлено", () -> {
            int sent = apiCalls(fake, "sendPhoto") + apiCalls(fake, "sendMessage");
            assertTrue(sent > 0, "меню не отправлено ни одним способом");
        });

        check("Без права: ошибок наружу не вылетело", () ->
                assertEquals(0, fake.errors().size()));

        check("Без права: флаг эмодзи снят", () ->
                assertFalse(Emoji.isAllowed(), "флаг остался включённым"));

        check("Без права: в отправленном нет <tg-emoji>", () -> {
            String text = fake.lastText();
            assertFalse(text.contains("<tg-emoji"),
                    "кастомные эмодзи так и остались: " + text);
            assertTrue(text.contains("Привет"), "текст меню потерялся");
        });

        check("Без права: кнопки рабочие, но без иконок", () -> {
            String sent = Json.write(fake.callsOf("sendMessage"));
            assertTrue(sent.contains("tariffs"), "кнопки потерялись");
            assertFalse(sent.contains("icon_custom_emoji_id"), "иконки остались");
        });

        check("Без права: повторный клик работает", () -> {
            int before = fake.callsOf("sendMessage").size() + fake.callsOf("sendPhoto").size();
            handler.handle(update("callback_query", callback(6101, "tariffs", 9001)));
            int after = fake.callsOf("sendMessage").size() + fake.callsOf("sendPhoto").size();
            assertTrue(after > before, "на клик нет ответа");
        });

        fake.rejectCustomEmoji(false);
        Emoji.resetForTests();

        // --- нередактируемое сообщение (старое меню с фото) ---
        check("Нет текста для правки: отправляется новое", () -> {
            fake.reset();
            fake.markNoText(9999);

            int before = fake.callsOf("sendMessage").size() + fake.callsOf("sendPhoto").size();
            handler.handle(update("callback_query", callback(9999, "settings", 9001)));
            int after = fake.callsOf("sendMessage").size() + fake.callsOf("sendPhoto").size();

            assertTrue(after > before, "новое сообщение не отправлено");
            assertTrue(fake.lastText().contains("Настройки"), "меню настроек не показано");
        });

        check("Нет текста для правки: try/catch не пропускает ошибку наружу", () ->
                assertEquals(0, fake.errors().size()));

        // --- stripHtml ---
        check("stripHtml: убирает теги, оставляет эмодзи", () -> {
            String html = "Привет " + Emoji.html("wave", "👋") + " и " + Emoji.html("gem", "💎");
            String plain = Emoji.stripHtml(html);
            assertFalse(plain.contains("tg-emoji"), "тег остался: " + plain);
            assertTrue(plain.contains("👋") && plain.contains("💎"), "эмодзи пропали: " + plain);
        });

        check("stripHtml: текст без тегов не меняется", () ->
                assertEquals("обычный текст", Emoji.stripHtml("обычный текст")));

        check("stripHtml: null безопасен", () ->
                assertEquals(null, Emoji.stripHtml(null)));

        // --- итог ---
        System.out.println();
        System.out.println("────────────────────────────────────────");
        System.out.println("✅ Пройдено: " + passed + "   ❌ Провалено: " + failed);
        System.out.println("────────────────────────────────────────");

        if (!fake.errors().isEmpty()) {
            System.out.println("Ошибки сервера:");
            fake.errors().forEach(error -> System.out.println("  " + error));
        }

        db.close();
        fake.stop();
        System.exit(failed == 0 ? 0 : 1);
    }

    // ------------------------------------------------------------ тестовые данные

    private static int apiCalls(FakeTelegram fake, String method) {
        return fake.callsOf(method).size();
    }

    /** Последний текст: обычное сообщение или подпись к фото. */
    private static String lastText(FakeTelegram fake) {
        return fake.lastText();
    }

    private static Map<String, Object> update(String field, Map<String, Object> payload) {
        Map<String, Object> update = new LinkedHashMap<>();
        update.put("update_id", System.nanoTime() % 1_000_000);
        update.put(field, payload);
        return update;
    }

    private static Map<String, Object> message(long messageId, long userId, String text) {
        Map<String, Object> from = new LinkedHashMap<>();
        from.put("id", userId);
        from.put("user_id", userId);
        from.put("is_bot", false);
        from.put("first_name", "Тест" + userId);
        from.put("username", "user" + userId);

        Map<String, Object> chat = new LinkedHashMap<>();
        chat.put("id", userId);
        chat.put("type", "private");

        Map<String, Object> message = new LinkedHashMap<>();
        message.put("message_id", messageId);
        message.put("from", from);
        message.put("chat", chat);
        message.put("date", System.currentTimeMillis() / 1000);
        message.put("text", text);
        return message;
    }

    private static Map<String, Object> payment(long messageId, long userId, String payload) {
        Map<String, Object> message = message(messageId, userId, "");
        message.remove("text");

        Map<String, Object> successful = new LinkedHashMap<>();
        successful.put("currency", "XTR");
        successful.put("total_amount", 75);
        successful.put("invoice_payload", payload);
        successful.put("telegram_payment_charge_id", "charge_1");
        successful.put("provider_payment_charge_id", "provider_1");
        message.put("successful_payment", successful);
        return message;
    }

    private static Map<String, Object> callback(long messageId, String data, long userId) {
        Map<String, Object> from = new LinkedHashMap<>();
        from.put("id", userId);
        from.put("user_id", userId);
        from.put("is_bot", false);
        from.put("first_name", "Тест" + userId);

        Map<String, Object> chat = new LinkedHashMap<>();
        chat.put("id", userId);
        chat.put("type", "private");

        Map<String, Object> message = new LinkedHashMap<>();
        message.put("message_id", messageId);
        message.put("chat", chat);
        message.put("date", System.currentTimeMillis() / 1000);

        Map<String, Object> query = new LinkedHashMap<>();
        query.put("id", "cb_" + System.nanoTime());
        query.put("from", from);
        query.put("message", message);
        query.put("chat_instance", "1");
        query.put("data", data);
        return query;
    }

    // ------------------------------------------------------------ утверждения

    private interface Body {
        void run() throws Exception;
    }

    private static void check(String name, Body body) {
        try {
            body.run();
            passed++;
            System.out.println("✅ " + name);
        } catch (Throwable error) {
            failed++;
            System.out.println("❌ " + name + " — " + error.getMessage());
        }
    }

    private static void assertEquals(Object expected, Object actual) {
        if (expected == null ? actual != null : !expected.equals(actual)) {
            throw new AssertionError("ожидалось «" + expected + "», получено «" + actual + "»");
        }
    }

    private static void assertTrue(boolean condition) {
        assertTrue(condition, "условие не выполнено");
    }

    private static void assertTrue(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError(message);
        }
    }

    private static void assertFalse(boolean condition, String message) {
        if (condition) {
            throw new AssertionError(message);
        }
    }

    private static void assertEquals(long expected, long actual) {
        if (expected != actual) {
            throw new AssertionError("ожидалось " + expected + ", получено " + actual);
        }
    }

    private static void assertEquals(int expected, int actual) {
        if (expected != actual) {
            throw new AssertionError("ожидалось " + expected + ", получено " + actual);
        }
    }
}
