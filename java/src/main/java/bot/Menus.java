package bot;

import static bot.Emoji.html;

import java.time.Instant;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Тексты меню. Кастомные эмодзи вставлены через HTML-тег {@code <tg-emoji>}.
 */
public final class Menus {

    private static final DateTimeFormatter DATE = DateTimeFormatter
            .ofPattern("dd.MM.yyyy HH:mm")
            .withZone(ZoneId.systemDefault());

    private static final DateTimeFormatter DATE_SECONDS = DateTimeFormatter
            .ofPattern("dd.MM.yyyy HH:mm:ss")
            .withZone(ZoneId.systemDefault());

    private Menus() {
    }

    /** Готовое меню: текст, клавиатура и ключ баннера (может отсутствовать). */
    public static final class Menu {
        private final String text;
        private final Map<String, Object> keyboard;
        private final String banner;

        public Menu(String text, Map<String, Object> keyboard, String banner) {
            this.text = text;
            this.keyboard = keyboard;
            this.banner = banner;
        }

        public Menu(String text, Map<String, Object> keyboard) {
            this(text, keyboard, null);
        }

        public String text() {
            return text;
        }

        public Map<String, Object> keyboard() {
            return keyboard;
        }

        public String banner() {
            return banner;
        }
    }

    public static String escape(String value) {
        if (value == null) {
            return "";
        }
        return value.replace("&", "&amp;")
                .replace("<", "&lt;")
                .replace(">", "&gt;")
                .replace("\"", "&quot;");
    }

    public static String nick(Map<String, Object> user) {
        if (user == null) {
            return "Пользователь";
        }
        String name = Json.getString(user, "first_name", "").isBlank()
                ? "Пользователь"
                : Json.getString(user, "first_name", "");
        String username = Json.getString(user, "username", "");
        return username.isBlank() ? escape(name) : escape(name) + " (@" + escape(username) + ")";
    }

    public static String subStatus(Map<String, Object> user, Config config) {
        if (user == null) {
            return "не активна";
        }
        if (Json.getLong(user, "is_banned", 0) == 1) {
            return "заблокирована (бан)";
        }
        if (config.isAdmin(Json.getLong(user, "user_id", 0))) {
            return "безлимит (админ)";
        }

        long until = Json.getLong(user, "subscription_until", 0);
        long nowSeconds = System.currentTimeMillis() / 1000;

        if (until > nowSeconds) {
            long days = (until - nowSeconds + 86399) / 86400;
            String premium = Json.getLong(user, "is_premium", 0) == 1 ? "Премиум " : "";
            String date = DATE.format(Instant.ofEpochSecond(until));
            return premium + "активна до " + date + " (" + days + " дн.)";
        }
        return "не активна";
    }

    public static String archiveDate(long seconds) {
        return seconds > 0 ? DATE_SECONDS.format(Instant.ofEpochSecond(seconds)) : "";
    }

    // ------------------------------------------------------------- главное

    public static Menu main(Map<String, Object> user, Config config, Database db) {
        long userId = Json.getLong(user, "user_id", 0);
        Map<String, Object> connection = db.getBusinessConnection(userId);
        boolean connected = connection != null
                && Json.getLong(connection, "is_enabled", 0) == 1;

        String text = html("wave", "👋") + " Привет, <b>" + nick(user) + "</b>!\n\n"
                + html("shield", "🛡") + " Статус подписки: <b>" + subStatus(user, config) + "</b>\n"
                + html("clip", "📎") + " Подключение: <b>"
                + (connected ? "подключён" : "не подключён") + "</b>\n\n"
                + html("arrow", "🔽") + " Выберите раздел:";

        return new Menu(text, Keyboards.main(userId, config), "main");
    }

    public static Menu settings(long userId, Database db) {
        Map<String, Object> user = db.getUser(userId);
        boolean deleted = user == null || Json.getLong(user, "notify_deleted", 1) == 1;
        boolean edited = user == null || Json.getLong(user, "notify_edited", 1) == 1;

        String text = html("gear", "⚙️") + " <b>Настройки</b>\n\n"
                + "• Уведомления об удалённых: <b>" + (deleted ? "включены" : "выключены") + "</b>\n"
                + "• Уведомления об изменённых: <b>" + (edited ? "включены" : "выключены") + "</b>\n"
                + "• Перехват сгорающих медиа (точка <code>.</code> или <code>,</code>): <b>активен</b>";

        return new Menu(text, Keyboards.settings(userId, db), "settings");
    }

    public static Menu connect(String botUsername) {
        String text = html("plug", "🔌") + " <b>Подключение к Telegram Business</b>\n\n"
                + "Подключение выполняет владелец аккаунта вручную — бот не может "
                + "сделать это сам.\n\n"
                + "1. Откройте <b>Профиль</b> → <b>Изменить</b>\n"
                + "2. Перейдите в <b>Автоматизация чатов</b>\n"
                + "3. Укажите юзернейм <code>@" + escape(botUsername) + "</code>\n"
                + "4. Нажмите <b>Добавить</b> и выдайте права на чтение и ответы\n\n"
                + "После этого нажмите «Проверить подключение».";
        return new Menu(text, Keyboards.connect());
    }

    public static Menu tariffs(long userId, Database db, Config config) {
        Map<String, Object> user = db.getUser(userId);
        String text = html("gem", "💎") + " <b>Тарифы</b>\n\n"
                + "Ваш статус: <b>" + subStatus(user, config) + "</b>\n\n"
                + "Оплата проходит звёздами Telegram. Подписка продлевается, "
                + "а не заменяет текущую.";
        return new Menu(text, Keyboards.tariffs(), "subscription");
    }

    public static Menu premium(long userId, Database db, Config config) {
        boolean premium = db.hasPremium(userId, config.adminIds);
        String text = html("star", "⭐️") + " <b>Премиум — максимальный контроль</b>\n\n"
                + "Статус: <b>" + (premium ? "активен ✅" : "не подключён ❌") + "</b>\n\n"
                + "Премиум оформляется на 365 дней.\n\n"
                + "Команды <code>.nah</code>, <code>.love</code>, <code>.bypass</code>, "
                + "<code>.mute</code> и <code>.unmute</code> работают и с обычной подпиской.";
        return new Menu(text, Keyboards.premium(), "subscription");
    }

    public static Menu invite(Map<String, Object> user, String botUsername) {
        long userId = Json.getLong(user, "user_id", 0);
        String link = "https://t.me/" + botUsername + "?start=ref_" + userId;

        String text = html("folder", "👥") + " <b>Пригласить друга</b>\n\n"
                + "• Вам за каждого друга: <b>+7 дней подписки</b>\n"
                + "• Другу при регистрации: <b>+3 дня подписки</b>\n\n"
                + "Приглашено: <b>" + Json.getLong(user, "referrals_count", 0) + "</b>\n\n"
                + "Ваша ссылка:\n<code>" + escape(link) + "</code>";
        return new Menu(text, Keyboards.invite());
    }

    public static Menu createBot(String botUsername) {
        String text = html("star", "🤖") + " <b>Создать бота</b>\n\n"
                + "1. Откройте <b>@BotFather</b> и отправьте <code>/newbot</code>\n"
                + "2. Придумайте имя и юзернейм\n"
                + "3. Скопируйте токен и вставьте его в <code>.env</code>:\n"
                + "   <code>BOT_TOKEN=123456:AA...</code>\n"
                + "4. Перезапустите бота\n\n"
                + "Этот бот уже работает как <code>@" + escape(botUsername) + "</code> — "
                + "если нужно, подключите его в Telegram Business.";
        return new Menu(text, Keyboards.createBot());
    }

    public static Menu admin(Database db) {
        Map<String, Long> stats = db.getStats();
        String text = html("crown", "👑") + " <b>Админ-панель</b>\n\n"
                + html("stats", "📊") + " Пользователей: <b>" + stats.get("totalUsers") + "</b>\n"
                + html("gem", "💎") + " Активных подписок: <b>" + stats.get("activeSubs") + "</b>\n"
                + html("star", "⭐️") + " Премиум: <b>" + stats.get("premiumUsers") + "</b>\n"
                + html("cross", "🚫") + " Забанено: <b>" + stats.get("bannedUsers") + "</b>\n"
                + html("plug", "🔌") + " Подключений: <b>" + stats.get("activeConnections") + "</b>\n\n"
                + "Выберите действие:";
        return new Menu(text, Keyboards.admin(), "admin");
    }

    public static Menu archiveTypes(String chatId, String title, List<String> selected) {
        String text = html("folder", "📁") + " <b>Настройка экспорта</b>\n"
                + html("user", "👤") + " Собеседник: <b>" + escape(title) + "</b> (<code>"
                + escape(chatId) + "</code>)\n\n"
                + "<b>Выберите типы сообщений:</b>";
        return new Menu(text, Keyboards.archiveTypes(selected));
    }

    public static Menu chatsList(List<Map<String, Object>> chats) {
        String text = html("folder", "📁") + " <b>Выберите чат для экспорта</b>\n\n"
                + "Найдено диалогов: <b>" + chats.size() + "</b>";
        return new Menu(text, Keyboards.chats(chats));
    }

    public static Menu emptyChats() {
        String text = html("folder", "📁") + " <b>Список чатов пуст</b>\n\n"
                + "В базе пока нет переписок. Подключите бота в Telegram Business "
                + "или импортируйте историю через /archive.";
        return new Menu(text, Keyboards.emptyChats());
    }

    public static Menu importWait() {
        String text = html("clip", "📥") + " <b>Импорт истории Telegram Desktop</b>\n\n"
                + "Отправьте файл <code>result.json</code> или ZIP-архив экспорта.";
        return new Menu(text, Keyboards.importWait());
    }

    public static Map<String, Object> menuToMessage(Menus.Menu menu) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("text", menu.text());
        if (menu.keyboard() != null) {
            payload.put("reply_markup", menu.keyboard());
        }
        return payload;
    }
}
