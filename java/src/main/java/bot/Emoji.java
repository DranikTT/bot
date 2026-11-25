package bot;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Кастомные (премиум) эмодзи Telegram.
 *
 * Возвращены в том виде, в котором ты их использовал раньше — через HTML-тег
 * {@code <tg-emoji>}: он работает и в тексте сообщений, и в подписях к фото.
 * Для кнопок тот же ID передаётся полем {@code icon_custom_emoji_id}.
 *
 * Требование Telegram: кастомные эмодзи в кнопках доступны только ботам,
 * у которых владелец имеет Telegram Premium (или куплен юзернейм на Fragment).
 * Если прав нет, Telegram не роняет запрос — просто показывает обычный эмодзи.
 */
public final class Emoji {

    private Emoji() {
    }

    /**
     * ID кастомных эмодзи из прежней версии (menuKeys.js + тексты menus.js).
     *
     * Значения взяты из истории репозитория, поэтому отображаются те же
     * картинки, что и раньше.
     */
    public static final Map<String, String> IDS = Map.ofEntries(
            Map.entry("wave", "5994750571041525522"),   // 👋
            Map.entry("back", "5877468380125990242"),   // 🔙
            Map.entry("check", "5776375003280838798"),  // ✅
            Map.entry("cross", "5778527486270770928"),  // ❌
            Map.entry("gear", "5877260593903175642"),   // ⚙️
            Map.entry("plug", "5877465816030515018"),   // 🔌 / 📢
            Map.entry("folder", "5877482652302315100"), // 📁 / 👥
            Map.entry("user", "5877477244938489129"),   // 👤
            Map.entry("gem", "6028530359975548369"),    // 💎
            Map.entry("star", "5879785854284599288"),   // ⭐️ / 🎁
            Map.entry("crown", "5807868868886009920"),  // 👑
            Map.entry("arrow", "5875008416132370818"),  // 🔽
            Map.entry("shield", "5778423822940114949"), // 🛡
            Map.entry("clip", "5877495434124988415"),   // 📎
            Map.entry("pin", "5796440171364749940"),    // 📌
            Map.entry("stats", "5931472654660800739")   // 📊
    );

    /**
     * Оборачивает обычный эмодзи в {@code <tg-emoji>} — так Telegram
     * подменяет его на кастомный.
     *
     * @param name ключ из {@link #IDS}
     * @param fallback обычный эмодзи, который виден без Premium и в уведомлениях
     */
    public static String html(String name, String fallback) {
        String id = IDS.get(name);
        if (id == null) {
            return fallback;
        }
        return "<tg-emoji emoji-id=\"" + id + "\">" + fallback + "</tg-emoji>";
    }

    /**
     * Проверка, что все ID действительно лежат в Telegram-диапазоне
     * (16-19 цифр, не ноль). Дешёвая страховка от опечатки в карте.
     */
    public static boolean isValidId(String id) {
        if (id == null || id.length() < 15 || id.length() > 20) {
            return false;
        }
        for (int i = 0; i < id.length(); i++) {
            if (!Character.isDigit(id.charAt(i))) {
                return false;
            }
        }
        return !id.matches("0+");
    }

    /** Возвращает кнопку с кастомным эмодзи и стилем. */
    public static Map<String, Object> button(String text, String callbackData, String emojiName, String style) {
        Map<String, Object> button = new LinkedHashMap<>();
        button.put("text", text);
        button.put("callback_data", callbackData);
        String id = IDS.get(emojiName);
        if (id != null) {
            button.put("icon_custom_emoji_id", id);
        }
        if (style != null) {
            button.put("style", style);
        }
        return button;
    }

    /** Кнопка-ссылка (например, «Открыть BotFather»). */
    public static Map<String, Object> urlButton(String text, String url, String emojiName, String style) {
        Map<String, Object> button = new LinkedHashMap<>();
        button.put("text", text);
        button.put("url", url);
        String id = IDS.get(emojiName);
        if (id != null) {
            button.put("icon_custom_emoji_id", id);
        }
        if (style != null) {
            button.put("style", style);
        }
        return button;
    }
}
