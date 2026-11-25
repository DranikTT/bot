package bot;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Сборка inline-клавиатур с защитой от «мёртвых» кнопок.
 *
 * Два правила, которые здесь зашиты:
 *  1. callback_data всегда передаётся отдельным полем (в прошлой версии он
 *     случайно попадал в текст, и все кнопки переставали работать);
 *  2. если Telegram отверг кастомные эмодзи в кнопках, клавиатура повторно
 *     отправляется без них — меню не должно пропадать.
 */
public final class Keyboard {

    private static final int MAX_CALLBACK_BYTES = 64;

    private final List<List<Map<String, Object>>> rows = new ArrayList<>();
    private List<Map<String, Object>> current = new ArrayList<>();

    public static Keyboard create() {
        return new Keyboard();
    }

    public Keyboard add(Map<String, Object> button) {
        Object data = button.get("callback_data");
        if (data != null) {
            String text = String.valueOf(button.get("text"));
            int bytes = data.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
            if (bytes == 0 || bytes > MAX_CALLBACK_BYTES) {
                throw new IllegalArgumentException("callback_data кнопки «" + text + "» неверной длины");
            }
            if (data.equals(text)) {
                throw new IllegalArgumentException("callback_data кнопки «" + text + "» совпадает с текстом");
            }
        }
        current.add(button);
        return this;
    }

    public Keyboard row() {
        if (!current.isEmpty()) {
            rows.add(current);
            current = new ArrayList<>();
        }
        return this;
    }

    public Keyboard row(Map<String, Object> button) {
        add(button);
        return row();
    }

    public Map<String, Object> build() {
        if (!current.isEmpty()) {
            rows.add(current);
            current = new ArrayList<>();
        }
        Map<String, Object> markup = new LinkedHashMap<>();
        markup.put("inline_keyboard", rows);
        return markup;
    }

    /**
     * Копия клавиатуры без кастомных эмодзи — запасной вариант,
     * когда Telegram не даёт боту их показывать.
     *
     * Принимает Object, потому что клавиатура в запросе может быть null.
     */
    @SuppressWarnings("unchecked")
    public static Object stripEmojis(Object rawMarkup) {
        if (!(rawMarkup instanceof Map)) {
            return null;
        }
        return stripEmojisMap((Map<String, Object>) rawMarkup);
    }

    public static Map<String, Object> stripEmojisMap(Map<String, Object> markup) {
        Map<String, Object> result = new LinkedHashMap<>();
        List<List<Map<String, Object>>> plainRows = new ArrayList<>();

        Object raw = markup.get("inline_keyboard");
        if (raw instanceof List) {
            for (Object rowObject : (List<?>) raw) {
                if (!(rowObject instanceof List)) {
                    continue;
                }
                List<Map<String, Object>> plainRow = new ArrayList<>();
                for (Object buttonObject : (List<?>) rowObject) {
                    if (buttonObject instanceof Map) {
                        Map<String, Object> copy = new LinkedHashMap<>();
                        for (Map.Entry<?, ?> entry : ((Map<?, ?>) buttonObject).entrySet()) {
                            if ("icon_custom_emoji_id".equals(entry.getKey())) {
                                continue;
                            }
                            copy.put(String.valueOf(entry.getKey()), entry.getValue());
                        }
                        plainRow.add(copy);
                    }
                }
                plainRows.add(plainRow);
            }
        }

        result.put("inline_keyboard", plainRows);
        return result;
    }
}
