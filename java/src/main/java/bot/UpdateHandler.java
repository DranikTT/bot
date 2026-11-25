package bot;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.logging.Level;
import java.util.logging.Logger;

/** Обработка всех апдейтов: сообщения, кнопки, платежи, бизнес-события. */
public final class UpdateHandler {

    private static final Logger LOG = Logger.getLogger(UpdateHandler.class.getName());

    private final TelegramApi api;
    private final Database db;
    private final Config config;
    private final MenuSender sender;

    /** Кто в каком сценарии: user_id -> действие. */
    private final Map<Long, String> states = new ConcurrentHashMap<>();
    /** Сессии экспорта: user_id -> данные. */
    private final Map<Long, ExportSession> exportSessions = new ConcurrentHashMap<>();
    /** Последнее медиа по чату — для «сгорающих» сообщений. */
    private final Map<String, Map<String, Object>> lastMedia = new ConcurrentHashMap<>();

    private String botUsername = "";

    /** Открытая сессия экспорта: выбранный чат и набор типов сообщений. */
    private static final class ExportSession {
        private final String chatId;
        private final String title;
        private final List<String> types;

        ExportSession(String chatId, String title, List<String> types) {
            this.chatId = chatId;
            this.title = title;
            this.types = types;
        }

        String chatId() {
            return chatId;
        }

        String title() {
            return title;
        }

        List<String> types() {
            return types;
        }
    }

    public UpdateHandler(TelegramApi api, Database db, Config config, MenuSender sender) {
        this.api = api;
        this.db = db;
        this.config = config;
        this.sender = sender;
    }

    public void setBotUsername(String username) {
        this.botUsername = username == null ? "" : username;
    }

    public void handle(Map<String, Object> update) {
        try {
            if (update.containsKey("callback_query")) {
                onCallback(Json.getObject(update, "callback_query"));
            } else if (update.containsKey("message")) {
                onMessage(Json.getObject(update, "message"));
            } else if (update.containsKey("pre_checkout_query")) {
                onPreCheckout(Json.getObject(update, "pre_checkout_query"));
            } else if (update.containsKey("inline_query")) {
                onInlineQuery(Json.getObject(update, "inline_query"));
            } else if (update.containsKey("business_connection")) {
                onBusinessConnection(Json.getObject(update, "business_connection"));
            } else if (update.containsKey("business_message")) {
                onBusinessMessage(Json.getObject(update, "business_message"));
            } else if (update.containsKey("edited_business_message")) {
                onEditedBusinessMessage(Json.getObject(update, "edited_business_message"));
            } else if (update.containsKey("deleted_business_messages")) {
                onDeletedBusinessMessages(Json.getObject(update, "deleted_business_messages"));
            }
        } catch (Exception error) {
            LOG.log(Level.WARNING, "Ошибка обработки апдейта", error);
        }
    }

    // ------------------------------------------------------------- кнопки

    private void onCallback(Map<String, Object> query) {
        if (query == null) {
            return;
        }
        String callbackId = Json.getString(query, "id", "");
        Map<String, Object> from = Json.getObject(query, "from");
        Map<String, Object> message = Json.getObject(query, "message");
        if (from == null || message == null) {
            api.answerCallbackQuery(callbackId, null, false);
            return;
        }

        long userId = Json.getLong(from, "user_id", Json.getLong(from, "id", 0));
        Map<String, Object> chat = Json.getObject(message, "chat");
        long chatId = chat == null ? userId : Json.getLong(chat, "id", userId);
        long messageId = Json.getLong(message, "message_id", 0);
        String data = Json.getString(query, "data", "");

        Map<String, Object> user = ensureUser(from);
        if (isBanned(user) && !config.isAdmin(userId)) {
            api.answerCallbackQuery(callbackId, "🚫 Доступ заблокирован", true);
            return;
        }

        try {
            routeCallback(data, callbackId, userId, chatId, messageId, user, from);
        } catch (TelegramApi.TelegramException error) {
            LOG.log(Level.WARNING, "Кнопка " + data, error);
            api.answerCallbackQuery(callbackId, "Ошибка: " + error.description(), true);
        } catch (Exception error) {
            LOG.log(Level.WARNING, "Кнопка " + data, error);
            api.answerCallbackQuery(callbackId, "Ошибка", true);
        }
    }

    private void routeCallback(String data, String callbackId, long userId, long chatId,
                               long messageId, Map<String, Object> user, Map<String, Object> from)
            throws Exception {
        if ("back_main".equals(data)) {
                    states.remove(userId);
                    sender.show(chatId, Menus.main(user, config, db), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("settings".equals(data)) {
                    sender.show(chatId, Menus.settings(userId, db), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("toggle_deleted".equals(data) || "toggle_edited".equals(data)) {
                    boolean deleted = "toggle_deleted".equals(data);
                    String field = deleted ? "notify_deleted" : "notify_edited";
                    boolean current = Json.getLong(user, field, 1) == 1;
                    db.setFlag(userId, field, !current);
                    sender.show(chatId, Menus.settings(userId, db), messageId, null);
                    api.answerCallbackQuery(callbackId,
                            (deleted ? "Удалённые: " : "Изменённые: ")
                                    + (!current ? "ВКЛ ✅" : "ВЫКЛ ❌"), false);
        }
        else if ("connect".equals(data)) {
                    sender.show(chatId, Menus.connect(botUsername), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("connect_help".equals(data)) {
                    sender.sendMessage(chatId, "📌 <b>Как подключить бота</b>\n\n"
                            + "1. Telegram → <b>Настройки</b> → <b>Telegram Business</b>\n"
                            + "2. <b>Чат-боты</b> → <b>Добавить бота</b>\n"
                            + "3. Введите <code>@" + Menus.escape(botUsername) + "</code>\n"
                            + "4. Разрешите чтение сообщений и ответы\n\n"
                            + "Готово — бот начнёт сохранять переписку.", null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("check_connection".equals(data)) {
                    Map<String, Object> connection = db.getBusinessConnection(userId);
                    if (connection != null && Json.getLong(connection, "is_enabled", 0) == 1) {
                        api.answerCallbackQuery(callbackId, "✅ Подключение активно", true);
                        sender.show(chatId, Menus.main(user, config, db), messageId, null);
                    } else {
                        api.answerCallbackQuery(callbackId, "❌ Подключение не найдено", true);
                    }
        }
        else if ("create_bot".equals(data)) {
                    sender.show(chatId, Menus.createBot(botUsername), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("create_bot_check".equals(data)) {
                    Map<String, Object> connection = db.getBusinessConnection(userId);
                    boolean connected = connection != null
                            && Json.getLong(connection, "is_enabled", 0) == 1;
                    api.answerCallbackQuery(callbackId,
                            connected ? "✅ Бот подключён" : "Бот ещё не подключён к Business", true);
        }
        else if ("tariffs".equals(data)) {
                    sender.show(chatId, Menus.tariffs(userId, db, config), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("premium".equals(data)) {
                    sender.show(chatId, Menus.premium(userId, db, config), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("buy_sub_7".equals(data)) {
                    invoice(callbackId, chatId, userId, "Подписка на 7 дней",
                        "Доступ ко всем функциям на 7 дней.", "sub_7", Keyboards.PRICE_7);
        }
        else if ("buy_sub_30".equals(data)) {
                    invoice(callbackId, chatId, userId, "Подписка на 30 дней",
                        "Доступ ко всем функциям на 30 дней.", "sub_30", Keyboards.PRICE_30);
        }
        else if ("buy_sub_90".equals(data)) {
                    invoice(callbackId, chatId, userId, "Подписка на 90 дней",
                        "Доступ ко всем функциям на 90 дней.", "sub_90", Keyboards.PRICE_90);
        }
        else if ("buy_sub_premium".equals(data)) {
                    invoice(callbackId, chatId, userId, "Премиум на 365 дней",
                        "Премиум-тариф и 365 дней подписки.", "sub_premium", Keyboards.PRICE_PREMIUM);
        }
        else if ("invite".equals(data)) {
                    sender.show(chatId, Menus.invite(user, botUsername), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("share_link".equals(data)) {
                    String link = "https://t.me/" + botUsername + "?start=ref_" + userId;
                    sender.sendMessage(chatId, "🔗 Ваша ссылка:\n<code>" + Menus.escape(link)
                            + "</code>\n\nПерешлите её другу — он получит +3 дня, вы +7 дней.", null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("invite_url".equals(data)) {
                    api.answerCallbackQuery(callbackId,
                        "https://t.me/" + botUsername + "?start=ref_" + userId, true);
        }
        else if ("admin".equals(data)) {
                    if (!config.isAdmin(userId)) {
                        api.answerCallbackQuery(callbackId, "⛔️ Нет доступа", true);
                        return;
                    }
                    sender.show(chatId, Menus.admin(db), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("admin_give_sub".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "give_sub",
                        "➕ <b>Выдача подписки</b>\n\nОтправьте ID и дни: <code>123456789 30</code>");
        }
        else if ("admin_take_sub".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "take_sub",
                        "➖ <b>Аннулирование подписки</b>\n\nОтправьте Telegram ID:");
        }
        else if ("admin_toggle_prem".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "toggle_prem",
                        "⭐️ <b>Выдать / снять Премиум</b>\n\nОтправьте Telegram ID:");
        }
        else if ("admin_ban_user".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "ban_user",
                        "🔨 <b>Бан</b>\n\nОтправьте Telegram ID:");
        }
        else if ("admin_unban_user".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "unban_user",
                        "🔓 <b>Разбан</b>\n\nОтправьте Telegram ID:");
        }
        else if ("admin_check_user".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "check_user",
                        "🔍 <b>Проверка юзера</b>\n\nОтправьте Telegram ID:");
        }
        else if ("admin_broadcast".equals(data)) {
                    adminPrompt(callbackId, chatId, userId, "broadcast",
                        "📢 <b>Рассылка</b>\n\nОтправьте текст — он уйдёт всем пользователям.");
        }
        else if ("export_chats".equals(data)) {
                    List<Map<String, Object>> chats = availableChats(userId);
                    sender.show(chatId, chats.isEmpty() ? Menus.emptyChats() : Menus.chatsList(chats),
                            messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("archive_import_cancel".equals(data)) {
                    states.remove(userId);
                    sender.show(chatId, Menus.main(user, config, db), messageId, null);
                    api.answerCallbackQuery(callbackId, "Импорт отменён", false);
        }
        else if ("archive_import_again".equals(data)) {
                    states.put(userId, "archive_wait_file");
                    sender.show(chatId, Menus.importWait(), messageId, null);
                    api.answerCallbackQuery(callbackId, null, false);
        }
        else if ("arch_cancel".equals(data)) {
                    exportSessions.remove(userId);
                    sender.show(chatId, Menus.main(user, config, db), messageId, null);
                    api.answerCallbackQuery(callbackId, "Отменено", false);
        }
        else if ("arch_start".equals(data)) {
                    startExport(callbackId, chatId, messageId, userId);
        }
        else if (data.startsWith("arch_sel_")) {
            selectChat(callbackId, chatId, messageId, userId, data.substring(9));
        }
        else if (data.startsWith("arch_toggle_")) {
            toggleType(callbackId, chatId, messageId, userId, data.substring(12));
        }
        else {
            api.answerCallbackQuery(callbackId, null, false);
        }
    }

    private void invoice(String callbackId, long chatId, long userId, String title,
                         String description, String payload, int amount) throws Exception {
        api.sendInvoice(chatId, title, description, payload + "_" + userId, amount);
        api.answerCallbackQuery(callbackId, null, false);
    }

    private void adminPrompt(String callbackId, long chatId, long userId, String action, String text) {
        if (!config.isAdmin(userId)) {
            api.answerCallbackQuery(callbackId, "⛔️ Нет доступа", true);
            return;
        }
        states.put(userId, "admin:" + action);
        sender.sendMessage(chatId, text, Keyboards.adminBack());
        api.answerCallbackQuery(callbackId, null, false);
    }

    // ----------------------------------------------------------- сообщения

    private void onMessage(Map<String, Object> message) {
        if (message == null) {
            return;
        }
        Map<String, Object> chat = Json.getObject(message, "chat");
        Map<String, Object> from = Json.getObject(message, "from");
        if (chat == null || from == null) {
            return;
        }
        if (!"private".equals(Json.getString(chat, "type", ""))) {
            return;
        }

        long userId = Json.getLong(from, "id", 0);
        long chatId = Json.getLong(chat, "id", userId);
        String text = Json.getString(message, "text", "");

        // Реферальная ссылка должна сработать ДО создания пользователя,
        // иначе «новый» юзер уже существует и бонус не начисляется.
        Map<String, Object> user = db.getUser(userId);
        if (user == null) {
            user = registerUser(from, userId, chatId, text);
            if (user == null) {
                return;
            }
        }

        if (isBanned(user) && !config.isAdmin(userId)) {
            sender.sendMessage(chatId, "🚫 Ваш доступ к боту заблокирован.", null);
            return;
        }

        if (message.containsKey("successful_payment")) {
            onPayment(message, userId, chatId);
            return;
        }
        if (message.containsKey("document")) {
            onDocument(message, userId, chatId, user);
            return;
        }

        if (text.startsWith("/")) {
            onCommand(message, text, userId, chatId, user);
            return;
        }

        if (".".equals(text.trim()) || ",".equals(text.trim())) {
            sendSelfDestruct(chatId, userId);
            return;
        }

        String state = states.get(userId);
        if (state != null && state.startsWith("admin:")) {
            onAdminInput(state.substring(6), text, userId, chatId);
            return;
        }
        if ("archive_wait_file".equals(state)) {
            sender.sendMessage(chatId, "Отправьте <code>result.json</code> или ZIP-архив.", null);
            return;
        }

        sender.show(chatId, Menus.main(user, config, db), null, null);
    }

    /** Создаёт пользователя, при необходимости засчитывая реферала. */
    private Map<String, Object> registerUser(Map<String, Object> from, long userId,
                                             long chatId, String text) {
        Long referredBy = parseReferral(text, userId);

        Object[] created = db.createUser(userId,
                Json.getString(from, "username", ""),
                Json.getString(from, "first_name", ""),
                referredBy);

        @SuppressWarnings("unchecked")
        Map<String, Object> user = (Map<String, Object>) created[0];
        boolean isNew = Boolean.TRUE.equals(created[1]);
        Long referrerId = (Long) created[2];

        if (isNew && referrerId != null) {
            try {
                sender.sendMessage(referrerId, "🎉 По вашей ссылке зарегистрировался <b>"
                        + Menus.escape(Json.getString(from, "first_name", "друг"))
                        + "</b>!\n✅ Начислено <b>+7 дней</b> подписки.", null);
            } catch (Exception ignored) {
                // пригласивший мог заблокировать бота
            }
            sender.sendMessage(chatId,
                    "🎁 Вы перешли по приглашению и получили <b>+3 дня</b> подписки!", null);
        }
        return user;
    }

    /** Достаёт ID пригласившего из /start ref_123. */
    private Long parseReferral(String text, long userId) {
        if (text == null || !text.startsWith("/start")) {
            return null;
        }
        String[] parts = text.split("\\s+");
        if (parts.length < 2 || !parts[1].startsWith("ref_")) {
            return null;
        }
        try {
            long candidate = Long.parseLong(parts[1].substring(4));
            if (candidate == userId) {
                return null;
            }
            Map<String, Object> referrer = db.getUser(candidate);
            if (referrer == null || Json.getLong(referrer, "is_banned", 0) == 1) {
                return null;
            }
            return candidate;
        } catch (NumberFormatException error) {
            return null;
        }
    }

    private void onCommand(Map<String, Object> message, String text, long userId,
                           long chatId, Map<String, Object> user) {
        String command = text.split("\\s+")[0].toLowerCase();
        String args = text.length() > command.length() ? text.substring(command.length()).trim() : "";

        if ("/start".equals(command)) {
            sender.show(chatId, Menus.main(user, config, db), null, null);
        }
        else if ("/menu".equals(command)) {
    sender.show(chatId, Menus.main(user, config, db), null, null);
        }
        else if ("/connect".equals(command)) {
    sender.show(chatId, Menus.connect(botUsername), null, null);
        }
        else if ("/archive".equals(command)) {
                    states.put(userId, "archive_wait_file");
                    sender.show(chatId, Menus.importWait(), null, null);
        }
        else if ("/export".equals(command)) {
                    if (!args.isBlank()) {
                        List<Map<String, Object>> chats = availableChats(userId);
                        for (Map<String, Object> chatItem : chats) {
                            if (args.equals(Json.getString(chatItem, "chat_id", ""))) {
                                startExportSession(chatId, userId, chatItem);
                                return;
                            }
                        }
                        sender.sendMessage(chatId,
                                "Этот чат не найден. Используйте /export без аргументов.", null);
                        return;
                    }
                    List<Map<String, Object>> chats = availableChats(userId);
                    if (chats.isEmpty()) {
                        sender.sendMessage(chatId, Menus.emptyChats().text(), Menus.emptyChats().keyboard());
                    } else {
                        sender.show(chatId, Menus.chatsList(chats), null, null);
                    }
        }
        else if ("/admin".equals(command)) {
                    if (!config.isAdmin(userId)) {
                        sender.sendMessage(chatId, "⛔️ У вас нет прав администратора.", null);
                        return;
                    }
                    sender.show(chatId, Menus.admin(db), null, null);
        }
        else {
            sender.sendMessage(chatId,
                    "Неизвестная команда. Доступно: /menu, /export, /archive, /connect, /admin.", null);
        }
    }

    // -------------------------------------------------------------- экспорт

    private List<Map<String, Object>> availableChats(long userId) {
        List<Map<String, Object>> result = new ArrayList<>();
        for (Map<String, Object> row : db.getOwnerChats(userId)) {
            String chatId = Json.getString(row, "chat_id", "");
            String peerName = db.decrypt(Json.getString(row, "peer_name", ""));
            String peerUsername = db.decrypt(Json.getString(row, "peer_username", ""));

            String title = peerName.isBlank()
                    ? (peerUsername.isBlank() ? "Чат " + chatId : "@" + peerUsername)
                    : (peerUsername.isBlank() ? peerName : peerName + " (@" + peerUsername + ")");

            Map<String, Object> chat = new LinkedHashMap<>();
            chat.put("chat_id", chatId);
            chat.put("title", title);
            result.add(chat);
        }
        return result;
    }

    private void selectChat(String callbackId, long chatId, long messageId, long userId, String target) {
        for (Map<String, Object> chat : availableChats(userId)) {
            if (target.equals(Json.getString(chat, "chat_id", ""))) {
                api.deleteMessage(chatId, messageId);
                startExportSession(chatId, userId, chat);
                api.answerCallbackQuery(callbackId, null, false);
                return;
            }
        }
        api.answerCallbackQuery(callbackId, "Чат не найден", true);
    }

    private void startExportSession(long chatId, long userId, Map<String, Object> chat) {
        String targetId = Json.getString(chat, "chat_id", "");
        String title = Json.getString(chat, "title", "Чат");

        exportSessions.put(userId, new ExportSession(
                targetId, title, new ArrayList<>(Keyboards.ARCHIVE_TYPES)));

        sender.show(chatId, Menus.archiveTypes(targetId, title, Keyboards.ARCHIVE_TYPES), null, null);
    }

    private void toggleType(String callbackId, long chatId, long messageId, long userId, String type) {
        ExportSession session = exportSessions.get(userId);
        if (session == null) {
            api.answerCallbackQuery(callbackId, "Сессия истекла", true);
            return;
        }

        List<String> types = new ArrayList<>(session.types());
        if (types.contains(type)) {
            types.remove(type);
        } else {
            types.add(type);
        }
        exportSessions.put(userId, new ExportSession(session.chatId(), session.title(), types));

        sender.show(chatId, Menus.archiveTypes(session.chatId(), session.title(), types), messageId, null);
        api.answerCallbackQuery(callbackId, null, false);
    }

    private void startExport(String callbackId, long chatId, long messageId, long userId) throws Exception {
        ExportSession session = exportSessions.get(userId);
        if (session == null) {
            api.answerCallbackQuery(callbackId, "Сессия истекла", true);
            return;
        }
        if (session.types().isEmpty()) {
            api.answerCallbackQuery(callbackId, "Выберите хотя бы один тип", true);
            return;
        }

        api.answerCallbackQuery(callbackId, "Собираю архив…", false);

        Archive.ExportResult result = Archive.export(db, userId, session.chatId(),
                session.title(), session.types(), config.maxImportedMediaMb * 1024 * 1024);

        if (result.count() == 0) {
            sender.sendMessage(chatId, "В этом чате нет сообщений выбранных типов.", null);
            return;
        }

        api.sendDocument(chatId, "chat_" + session.chatId() + ".html",
                result.html().getBytes(java.nio.charset.StandardCharsets.UTF_8),
                "📦 <b>HTML-архив готов</b>\nСообщений: <b>" + result.count() + "</b>", null);

        exportSessions.remove(userId);
        sender.show(chatId, Menus.main(ensureUserById(userId), config, db), messageId, null);
    }

    // --------------------------------------------------------------- импорт

    private void onDocument(Map<String, Object> message, long userId, long chatId,
                            Map<String, Object> user) {
        Map<String, Object> document = Json.getObject(message, "document");
        String fileName = Json.getString(document, "file_name", "").toLowerCase();
        boolean isArchive = fileName.endsWith(".json") || fileName.endsWith(".zip");

        String state = states.get(userId);
        if (!"archive_wait_file".equals(state)) {
            if (isArchive) {
                sender.sendMessage(chatId,
                        "Для импорта сначала отправьте /archive, затем файл.", null);
            }
            return;
        }
        if (!isArchive) {
            sender.sendMessage(chatId, "Нужен файл <code>result.json</code> или ZIP.", null);
            return;
        }

        long size = Json.getLong(document, "file_size", 0);
        if (size > config.maxImportFileMb * 1024 * 1024) {
            states.remove(userId);
            sender.sendMessage(chatId, "Файл больше " + config.maxImportFileMb + " МБ.", null);
            return;
        }

        states.remove(userId);
        Map<String, Object> status = api.sendMessage(chatId, "📥 Скачиваю и импортирую…", null, null, true);

        Path workdir = null;
        try {
            workdir = Files.createTempDirectory(config.TEMP_DIR, "import_");
            String fileId = Json.getString(document, "file_id", "");
            Map<String, Object> fileInfo = api.getFile(fileId);
            String filePath = Json.getString(fileInfo, "file_path", "");

            Path local = workdir.resolve(fileName.endsWith(".zip") ? "import.zip" : "result.json");
            Files.write(local, api.download(filePath));

            Path jsonPath = fileName.endsWith(".zip")
                    ? Archive.extractZip(local, workdir.resolve("extracted"))
                    : local;

            int imported = Archive.importDesktop(jsonPath, db, userId);

            long statusId = Json.getLong(status, "message_id", 0);
            api.editMessageText(chatId, statusId,
                    "✅ <b>Импорт завершён</b>\n\n• Добавлено сообщений: <b>" + imported + "</b>\n\n"
                            + "Для архива используйте /export.",
                    Keyboards.importResult(true));

        } catch (Exception error) {
            LOG.log(Level.WARNING, "Импорт", error);
            long statusId = Json.getLong(status, "message_id", 0);
            try {
                api.editMessageText(chatId, statusId,
                        "❌ <b>Ошибка импорта:</b> <code>" + Menus.escape(error.getMessage())
                                + "</code>", Keyboards.importResult(false));
            } catch (Exception ignored) {
                sender.sendMessage(chatId, "❌ Ошибка импорта: " + Menus.escape(error.getMessage()), null);
            }
        } finally {
            deleteRecursively(workdir);
        }
    }

    private static void deleteRecursively(Path path) {
        if (path == null) {
            return;
        }
        try (var stream = Files.walk(path)) {
            stream.sorted(java.util.Comparator.reverseOrder()).forEach(p -> {
                try {
                    Files.deleteIfExists(p);
                } catch (Exception ignored) {
                    // не критично
                }
            });
        } catch (Exception ignored) {
            // не критично
        }
    }

    // ----------------------------------------------------------- админ-ввод

    private void onAdminInput(String action, String text, long userId, long chatId) {
        String input = text.trim();

        if ("broadcast".equals(action)) {
            states.remove(userId);
            broadcast(chatId, input);
            return;
        }

        if ("give_sub".equals(action)) {
            String[] parts = input.split("\\s+");
            long targetId;
            int days = 30;
            try {
                targetId = Long.parseLong(parts[0]);
                if (parts.length > 1) {
                    days = Integer.parseInt(parts[1]);
                }
            } catch (Exception error) {
                sender.sendMessage(chatId, "Формат: <code>123456789 30</code>", null);
                return;
            }
            ensureUserById(targetId);
            db.addSubscriptionDays(targetId, days, false);
            states.remove(userId);
            sender.sendMessage(chatId, "✅ Пользователю <code>" + targetId
                    + "</code> начислено <b>" + days + " дн.</b>", Keyboards.adminBack());
            try {
                sender.sendMessage(targetId, "💎 Вам выдана подписка на <b>" + days + " дн.</b>", null);
            } catch (Exception ignored) {
                // пользователь мог заблокировать бота
            }
            return;
        }

        long targetId;
        try {
            targetId = Long.parseLong(input);
        } catch (NumberFormatException error) {
            sender.sendMessage(chatId, "Введите числовой Telegram ID.", null);
            return;
        }

        states.remove(userId);

        if ("toggle_prem".equals(action)) {
                    ensureUserById(targetId);
                    Map<String, Object> target = db.getUser(targetId);
                    boolean next = target == null || Json.getLong(target, "is_premium", 0) != 1;
                    db.setPremium(targetId, next);
                    sender.sendMessage(chatId, "⭐️ Премиум для <code>" + targetId + "</code>: <b>"
                            + (next ? "выдан ✅" : "снят ❌") + "</b>", Keyboards.adminBack());
        }
        else if ("ban_user".equals(action)) {
                    db.setBan(targetId, true);
                    sender.sendMessage(chatId, "🔨 Пользователь <code>" + targetId
                            + "</code> забанен.", Keyboards.adminBack());
        }
        else if ("unban_user".equals(action)) {
                    db.setBan(targetId, false);
                    sender.sendMessage(chatId, "🔓 Пользователь <code>" + targetId
                            + "</code> разбанен.", Keyboards.adminBack());
        }
        else if ("take_sub".equals(action)) {
                    db.resetSubscription(targetId);
                    sender.sendMessage(chatId, "➖ Подписка <code>" + targetId
                            + "</code> аннулирована.", Keyboards.adminBack());
        }
        else if ("check_user".equals(action)) {
                    Map<String, Object> target = db.getUser(targetId);
                    if (target == null) {
                        sender.sendMessage(chatId, "❌ Пользователь <code>" + targetId
                                + "</code> не найден.", Keyboards.adminBack());
                        return;
                    }
                    Map<String, Object> connection = db.getBusinessConnection(targetId);
                    boolean connected = connection != null
                            && Json.getLong(connection, "is_enabled", 0) == 1;
    
                    sender.sendMessage(chatId, "👤 <b>Досье</b> <code>" + targetId + "</code>\n\n"
                            + "• Имя: <b>" + Menus.nick(target) + "</b>\n"
                            + "• Подписка: <b>" + Menus.subStatus(target, config) + "</b>\n"
                            + "• Премиум: <b>" + (Json.getLong(target, "is_premium", 0) == 1 ? "да ⭐️" : "нет") + "</b>\n"
                            + "• Бан: <b>" + (Json.getLong(target, "is_banned", 0) == 1 ? "да 🚫" : "нет") + "</b>\n"
                            + "• Рефералов: <b>" + Json.getLong(target, "referrals_count", 0) + "</b>\n"
                            + "• Business: <b>" + (connected ? "подключён ✅" : "нет ❌") + "</b>",
                            Keyboards.adminBack());
        }
        else {
        }
    }

    private void broadcast(long chatId, String text) {
        List<Map<String, Object>> users = db.getAllUsers();
        Map<String, Object> status = api.sendMessage(chatId,
                "📢 Рассылка по <b>" + users.size() + "</b> пользователям…", null, null, true);

        int sent = 0;
        int failed = 0;
        for (Map<String, Object> user : users) {
            long targetId = Json.getLong(user, "user_id", 0);
            if (targetId <= 0) {
                continue;
            }
            try {
                sender.sendMessage(targetId, text, null);
                sent++;
            } catch (Exception error) {
                failed++;
            }
            try {
                Thread.sleep(40);
            } catch (InterruptedException error) {
                Thread.currentThread().interrupt();
                break;
            }
        }

        long statusId = Json.getLong(status, "message_id", 0);
        try {
            api.editMessageText(chatId, statusId, "✅ <b>Рассылка завершена</b>\n\n"
                    + "• Доставлено: <b>" + sent + "</b>\n• Ошибок: <b>" + failed + "</b>",
                    Keyboards.adminBack());
        } catch (Exception ignored) {
            sender.sendMessage(chatId, "Рассылка: доставлено " + sent + ", ошибок " + failed,
                    Keyboards.adminBack());
        }
    }

    // -------------------------------------------------------------- платежи

    private void onPreCheckout(Map<String, Object> query) {
        if (query != null) {
            api.answerPreCheckoutQuery(Json.getString(query, "id", ""));
        }
    }

    private void onPayment(Map<String, Object> message, long userId, long chatId) {
        Map<String, Object> payment = Json.getObject(message, "successful_payment");
        String payload = Json.getString(payment, "invoice_payload", "");
        String key = payload.contains("_") ? payload.substring(0, payload.lastIndexOf('_')) : payload;

        int days = 30;
        if ("sub_7".equals(key)) {
            days = 7;
        } else if ("sub_90".equals(key)) {
            days = 90;
        } else if ("sub_premium".equals(key)) {
            days = 365;
        }
        boolean premium = "sub_premium".equals(key);

        try {
            db.addSubscriptionDays(userId, days, premium);
        } catch (Exception error) {
            LOG.log(Level.WARNING, "Платёж", error);
            sender.sendMessage(chatId, "❌ Оплата получена, но подписку обновить не удалось.", null);
            return;
        }

        sender.sendMessage(chatId, "✅ Оплата получена! Начислено <b>" + days + " дней</b>"
                + (premium ? " и Премиум ⭐️" : "") + ".", null);
        sender.show(chatId, Menus.main(ensureUserById(userId), config, db), null, null);
    }

    // --------------------------------------------------------- inline-режим

    private void onInlineQuery(Map<String, Object> query) {
        if (query == null) {
            return;
        }
        String queryId = Json.getString(query, "id", "");
        Map<String, Object> from = Json.getObject(query, "from");
        long userId = from == null ? 0 : Json.getLong(from, "id", 0);
        String link = "https://t.me/" + botUsername + "?start=ref_" + userId;

        Map<String, Object> content = new LinkedHashMap<>();
        content.put("message_text", "👋 Я пользуюсь ботом, который сохраняет удалённые "
                + "и изменённые сообщения в Telegram.\n\n🎁 Переходи по ссылке и забери бонус: " + link);
        content.put("link_preview_options", Map.of("is_disabled", false));

        Map<String, Object> result = new LinkedHashMap<>();
        result.put("type", "article");
        result.put("id", "ref_" + userId);
        result.put("title", "Пригласить друга и получить +7 дней");
        result.put("description", "Друг получит +3 дня при регистрации");
        result.put("input_message_content", content);
        result.put("reply_markup", Map.of("inline_keyboard",
                List.of(List.of(Emoji.urlButton("🚀 Подключить бота (+3 дня)", link, "plug", "success")))));

        try {
            api.answerInlineQuery(queryId, List.of(result));
        } catch (Exception error) {
            LOG.fine("inline_query: " + error.getMessage());
        }
    }

    // ------------------------------------------------------- бизнес-события

    private void onBusinessConnection(Map<String, Object> connection) {
        if (connection == null) {
            return;
        }
        Map<String, Object> user = Json.getObject(connection, "user");
        long ownerId = user == null ? 0 : Json.getLong(user, "id", 0);
        String connectionId = Json.getString(connection, "id", "");
        boolean enabled = Json.getBoolean(connection, "is_enabled", false);

        db.saveBusinessConnection(ownerId, connectionId, enabled);
        LOG.info("[Business] " + ownerId + " → " + connectionId + " (" + enabled + ")");
    }

    private void onBusinessMessage(Map<String, Object> message) {
        if (message == null) {
            return;
        }
        String connectionId = Json.getString(message, "business_connection_id", "");
        if (connectionId.isBlank()) {
            return;
        }
        long ownerId = ownerOf(connectionId);
        if (ownerId == 0) {
            return;
        }

        Map<String, Object> chat = Json.getObject(message, "chat");
        Map<String, Object> from = Json.getObject(message, "from");
        long chatId = chat == null ? 0 : Json.getLong(chat, "id", 0);

        String[] media = mediaOf(message);
        String text = Json.getString(message, "text", Json.getString(message, "caption", ""));

        db.saveMessage(
                String.valueOf(chatId),
                Json.getLong(message, "message_id", 0),
                ownerId,
                from == null ? null : Json.getLong(from, "id", 0),
                from == null ? "" : Json.getString(from, "first_name", ""),
                from == null ? "" : Json.getString(from, "username", ""),
                peerTitle(chat),
                chat == null ? "" : Json.getString(chat, "username", ""),
                text,
                media[0],
                media[1],
                media[2],
                from != null && Json.getLong(from, "id", 0) == ownerId,
                Json.getLong(message, "date", System.currentTimeMillis() / 1000));

        if (media[1] != null) {
            Map<String, Object> entry = new LinkedHashMap<>();
            entry.put("chat_id", chatId);
            entry.put("message_id", Json.getLong(message, "message_id", 0));
            entry.put("media_type", media[0]);
            lastMedia.put(ownerId + "_" + chatId, entry);
        }
    }

    private void onEditedBusinessMessage(Map<String, Object> message) {
        if (message == null) {
            return;
        }
        Map<String, Object> chat = Json.getObject(message, "chat");
        if (chat == null) {
            return;
        }
        db.saveEditedText(String.valueOf(Json.getLong(chat, "id", 0)),
                Json.getLong(message, "message_id", 0),
                Json.getString(message, "text", Json.getString(message, "caption", "")));
    }

    private void onDeletedBusinessMessages(Map<String, Object> event) {
        if (event == null) {
            return;
        }
        Map<String, Object> chat = Json.getObject(event, "chat");
        List<Object> ids = Json.getArray(event, "message_ids");
        if (chat == null || ids == null) {
            return;
        }
        List<Long> messageIds = new ArrayList<>();
        for (Object id : ids) {
            if (id instanceof Number) {
                messageIds.add(((Number) id).longValue());
            }
        }
        db.markDeleted(String.valueOf(Json.getLong(chat, "id", 0)), messageIds);
    }

    private void sendSelfDestruct(long chatId, long userId) {
        Map<String, Object> entry = lastMedia.get(userId + "_" + chatId);
        if (entry == null) {
            sender.sendMessage(chatId, "Пока нечего отправлять: медиа появится здесь, "
                    + "когда бот зафиксирует сгорающее сообщение.", null);
            return;
        }
        try {
            api.copyMessage(chatId,
                    Json.getLong(entry, "chat_id", chatId),
                    Json.getLong(entry, "message_id", 0));
            sender.sendMessage(chatId, "📤 Отправил сохранённое медиа.", null);
        } catch (Exception error) {
            sender.sendMessage(chatId, "Не удалось переслать медиа — оно могло исчезнуть.", null);
        }
    }

    private long ownerOf(String connectionId) {
        Map<String, Object> connection = db.getConnectionById(connectionId);
        return connection == null ? 0 : Json.getLong(connection, "user_id", 0);
    }

    private static String peerTitle(Map<String, Object> chat) {
        if (chat == null) {
            return "";
        }
        String title = Json.getString(chat, "title", "");
        if (!title.isBlank()) {
            return title;
        }
        String name = (Json.getString(chat, "first_name", "") + " "
                + Json.getString(chat, "last_name", "")).trim();
        if (!name.isBlank()) {
            return name;
        }
        String username = Json.getString(chat, "username", "");
        return username.isBlank() ? "" : "@" + username;
    }

    private static String[] mediaOf(Map<String, Object> message) {
        if (message.containsKey("photo")) {
            List<Map<String, Object>> photos = Json.getObjectList(message, "photo");
            if (photos != null && !photos.isEmpty()) {
                return new String[]{"photo",
                        Json.getString(photos.get(photos.size() - 1), "file_id", ""), null};
            }
        }
        if (message.containsKey("video")) {
            return new String[]{"video", fileId(message, "video"), fileName(message, "video")};
        }
        if (message.containsKey("video_note")) {
            return new String[]{"video_note", fileId(message, "video_note"), null};
        }
        if (message.containsKey("animation")) {
            return new String[]{"animation", fileId(message, "animation"), null};
        }
        if (message.containsKey("voice")) {
            return new String[]{"voice", fileId(message, "voice"), null};
        }
        if (message.containsKey("audio")) {
            return new String[]{"audio", fileId(message, "audio"), null};
        }
        if (message.containsKey("sticker")) {
            return new String[]{"sticker", fileId(message, "sticker"), null};
        }
        if (message.containsKey("document")) {
            return new String[]{"document", fileId(message, "document"), fileName(message, "document")};
        }
        return new String[]{null, null, null};
    }

    private static String fileId(Map<String, Object> message, String field) {
        Map<String, Object> media = Json.getObject(message, field);
        return media == null ? null : Json.getString(media, "file_id", null);
    }

    private static String fileName(Map<String, Object> message, String field) {
        Map<String, Object> media = Json.getObject(message, field);
        return media == null ? null : Json.getString(media, "file_name", null);
    }

    // ------------------------------------------------------------- утилиты

    private Map<String, Object> ensureUser(Map<String, Object> from) {
        long userId = Json.getLong(from, "id", Json.getLong(from, "user_id", 0));
        Map<String, Object> user = db.getUser(userId);
        if (user != null) {
            return user;
        }
        Object[] created = db.createUser(userId,
                Json.getString(from, "username", ""),
                Json.getString(from, "first_name", ""), null);
        @SuppressWarnings("unchecked")
        Map<String, Object> result = (Map<String, Object>) created[0];
        return result != null ? result : Map.of("user_id", userId);
    }

    private Map<String, Object> ensureUserById(long userId) {
        Map<String, Object> user = db.getUser(userId);
        return user != null ? user : Map.of("user_id", userId);
    }

    private boolean isBanned(Map<String, Object> user) {
        return Json.getLong(user, "is_banned", 0) == 1;
    }
}
