package bot;

import static bot.Emoji.button;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * Все inline-клавиатуры.
 *
 * Дизайн: короткие подписи, по две кнопки в ряд, акцентные кнопки отдельными
 * рядами. Каждая кнопка получает кастомный эмодзи и стиль
 * (primary / success / danger) — Telegram поддерживает это начиная с Bot API 9.x.
 */
public final class Keyboards {

    public static final int PRICE_7 = 25;
    public static final int PRICE_30 = 75;
    public static final int PRICE_90 = 150;
    public static final int PRICE_PREMIUM = 300;

    public static final List<String> ARCHIVE_TYPES = List.of(
            "text", "photo", "video", "audio", "document",
            "animation", "video_note", "voice", "location", "sticker");

    private static final Map<String, String> TYPE_LABELS = Map.ofEntries(
            Map.entry("text", "💬 Текст"),
            Map.entry("photo", "🖼 Фото"),
            Map.entry("video", "🎬 Видео"),
            Map.entry("audio", "🎵 Аудио"),
            Map.entry("document", "📎 Файлы"),
            Map.entry("animation", "🎞 Гиф"),
            Map.entry("video_note", "🔘 Кружки"),
            Map.entry("voice", "🎤 Голос"),
            Map.entry("location", "📍 Гео"),
            Map.entry("sticker", "✨ Стикеры"));

    private Keyboards() {
    }

    public static String typeLabel(String type) {
        return TYPE_LABELS.getOrDefault(type, type);
    }

    /** Главное меню — ровно тот дизайн, который ты прислал. */
    public static Map<String, Object> main(long userId, Config config) {
        Keyboard keyboard = Keyboard.create()
                .add(button("Тарифы", "tariffs", "gem", "primary"))
                .add(button("Пригласить друга", "invite", "folder", "primary"))
                .row()
                .add(button("Подключение", "connect", "plug", "primary"))
                .add(button("Создать бота", "create_bot", "star", "success"))
                .row()
                .add(button("Настройки", "settings", "gear", "primary"))
                .row();

        if (config.isAdmin(userId)) {
            keyboard.add(button("Админ-панель", "admin", "crown", "danger")).row();
        }

        return keyboard.build();
    }

    public static Map<String, Object> settings(long userId, Database db) {
        Map<String, Object> user = db.getUser(userId);
        boolean deleted = user == null || Json.getLong(user, "notify_deleted", 1) == 1;
        boolean edited = user == null || Json.getLong(user, "notify_edited", 1) == 1;

        return Keyboard.create()
                .row(button("Удалённые: " + (deleted ? "ВКЛ" : "ВЫКЛ"), "toggle_deleted",
                        deleted ? "check" : "cross", deleted ? "success" : "danger"))
                .row(button("Изменённые: " + (edited ? "ВКЛ" : "ВЫКЛ"), "toggle_edited",
                        edited ? "check" : "cross", edited ? "success" : "danger"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> connect() {
        return Keyboard.create()
                .row(button("Проверить подключение", "check_connection", "check", "success"))
                .row(button("Как подключить", "connect_help", "clip", "primary"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> tariffs() {
        return Keyboard.create()
                .add(button("7 дней — " + PRICE_7 + " ⭐", "buy_sub_7", "gem", "primary"))
                .add(button("30 дней — " + PRICE_30 + " ⭐", "buy_sub_30", "gem", "primary"))
                .row()
                .row(button("90 дней — " + PRICE_90 + " ⭐", "buy_sub_90", "gem", "primary"))
                .row(button("Премиум", "premium", "star", "success"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> premium() {
        return Keyboard.create()
                .row(button("Купить Премиум — " + PRICE_PREMIUM + " ⭐", "buy_sub_premium", "star", "success"))
                .row(button("Все тарифы", "tariffs", "gem", "primary"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> invite() {
        return Keyboard.create()
                .row(button("Поделиться ссылкой", "share_link", "folder", "success"))
                .row(button("Открыть в Telegram", "invite_url", "plug", "primary"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> createBot() {
        return Keyboard.create()
                .row(Emoji.urlButton("Открыть BotFather", "https://t.me/BotFather", "star", "success"))
                .row(button("Проверить подключение", "create_bot_check", "check", "primary"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> archiveTypes(List<String> selected) {
        Keyboard keyboard = Keyboard.create();

        for (int index = 0; index < ARCHIVE_TYPES.size(); index += 2) {
            for (int inner = index; inner < Math.min(index + 2, ARCHIVE_TYPES.size()); inner++) {
                String type = ARCHIVE_TYPES.get(inner);
                boolean active = selected.contains(type);
                keyboard.add(button(typeLabel(type), "arch_toggle_" + type,
                        active ? "check" : "cross", active ? "success" : "primary"));
            }
            keyboard.row();
        }

        return keyboard
                .add(button("Отменить", "arch_cancel", "cross", "danger"))
                .add(button("Начать экспорт", "arch_start", "check", "success"))
                .build();
    }

    public static Map<String, Object> admin() {
        return Keyboard.create()
                .add(button("Выдать подписку", "admin_give_sub", "check", "success"))
                .add(button("Забрать подписку", "admin_take_sub", "cross", "danger"))
                .row()
                .add(button("Выдать/снять Премиум", "admin_toggle_prem", "star", "success"))
                .add(button("Проверить юзера", "admin_check_user", "user", "primary"))
                .row()
                .add(button("Забанить", "admin_ban_user", "cross", "danger"))
                .add(button("Разбанить", "admin_unban_user", "check", "success"))
                .row()
                .row(button("Рассылка всем", "admin_broadcast", "plug", "primary"))
                .row(back("Назад в меню"))
                .build();
    }

    public static Map<String, Object> chats(List<Map<String, Object>> chats) {
        Keyboard keyboard = Keyboard.create();

        for (Map<String, Object> chat : chats.subList(0, Math.min(chats.size(), 35))) {
            String chatId = Json.getString(chat, "chat_id", "");
            String title = Json.getString(chat, "title", "Чат " + chatId);
            if (title.length() > 48) {
                title = title.substring(0, 48);
            }
            keyboard.row(button(title, "arch_sel_" + chatId, "user", "primary"));
        }

        keyboard.row(back("Назад"));
        return keyboard.build();
    }

    public static Map<String, Object> emptyChats() {
        return Keyboard.create()
                .row(button("Подключить бота", "connect", "plug", "primary"))
                .row(back("Назад"))
                .build();
    }

    public static Map<String, Object> importWait() {
        return Keyboard.create()
                .row(button("Отмена", "archive_import_cancel", "cross", "danger"))
                .build();
    }

    public static Map<String, Object> importResult(boolean ok) {
        Keyboard keyboard = Keyboard.create();
        if (!ok) {
            keyboard.row(button("Повторить импорт", "archive_import_again", "arrow", "primary"));
        }
        keyboard.row(back("Назад в меню"));
        return keyboard.build();
    }

    public static Map<String, Object> adminBack() {
        return Keyboard.create().row(back("Назад в админ-панель")).build();
    }

    private static Map<String, Object> back(String label) {
        return button(label, "back_main", "back", "primary");
    }

    public static List<String> defaultArchiveTypes() {
        return new ArrayList<>(ARCHIVE_TYPES);
    }
}
