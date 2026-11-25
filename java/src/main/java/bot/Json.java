package bot;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Минимальный JSON-парсер/сериализатор без зависимостей.
 *
 * Нужен потому, что Bot API отвечает JSON, а сторонние библиотеки тянуть
 * не хочется. Поддерживает всё, что реально встречается в ответах Telegram:
 * объекты, массивы, строки с escape-последовательностями и unicode-эскейпами,
 * числа, true/false/null.
 */
public final class Json {

    private Json() {
    }

    // ---------------------------------------------------------------- парсинг

    public static Object parse(String text) {
        Parser parser = new Parser(text);
        Object value = parser.readValue();
        parser.skipWhitespace();
        return value;
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> parseObject(String text) {
        Object value = parse(text);
        if (value instanceof Map) {
            return (Map<String, Object>) value;
        }
        throw new IllegalArgumentException("Ожидался JSON-объект");
    }

    private static final class Parser {
        private final String text;
        private int pos;

        Parser(String text) {
            this.text = text == null ? "" : text;
        }

        Object readValue() {
            skipWhitespace();
            if (pos >= text.length()) {
                return null;
            }
            char ch = text.charAt(pos);
            switch (ch) {
                case '{':
                    return readObject();
                case '[':
                    return readArray();
                case '"':
                    return readString();
                case 't':
                    expect("true");
                    return Boolean.TRUE;
                case 'f':
                    expect("false");
                    return Boolean.FALSE;
                case 'n':
                    expect("null");
                    return null;
                default:
                    return readNumber();
            }
        }

        private Map<String, Object> readObject() {
            Map<String, Object> result = new LinkedHashMap<>();
            pos++; // {
            skipWhitespace();
            if (peek() == '}') {
                pos++;
                return result;
            }
            while (pos < text.length()) {
                skipWhitespace();
                String key = readString();
                skipWhitespace();
                if (peek() == ':') {
                    pos++;
                }
                result.put(key, readValue());
                skipWhitespace();
                char ch = peek();
                pos++;
                if (ch == '}') {
                    break;
                }
                if (ch != ',') {
                    break;
                }
            }
            return result;
        }

        private List<Object> readArray() {
            List<Object> result = new ArrayList<>();
            pos++; // [
            skipWhitespace();
            if (peek() == ']') {
                pos++;
                return result;
            }
            while (pos < text.length()) {
                result.add(readValue());
                skipWhitespace();
                char ch = peek();
                pos++;
                if (ch == ']') {
                    break;
                }
                if (ch != ',') {
                    break;
                }
            }
            return result;
        }

        private String readString() {
            StringBuilder builder = new StringBuilder();
            pos++; // открывающая кавычка
            while (pos < text.length()) {
                char ch = text.charAt(pos++);
                if (ch == '"') {
                    break;
                }
                if (ch != '\\') {
                    builder.append(ch);
                    continue;
                }
                char escaped = text.charAt(pos++);
                switch (escaped) {
                    case '"': builder.append('"'); break;
                    case '\\': builder.append('\\'); break;
                    case '/': builder.append('/'); break;
                    case 'b': builder.append('\b'); break;
                    case 'f': builder.append('\f'); break;
                    case 'n': builder.append('\n'); break;
                    case 'r': builder.append('\r'); break;
                    case 't': builder.append('\t'); break;
                    case 'u':
                        String hex = text.substring(pos, Math.min(pos + 4, text.length()));
                        builder.append((char) Integer.parseInt(hex, 16));
                        pos += 4;
                        break;
                    default:
                        builder.append(escaped);
                }
            }
            return builder.toString();
        }

        private Number readNumber() {
            int start = pos;
            while (pos < text.length()) {
                char ch = text.charAt(pos);
                if (Character.isDigit(ch) || ch == '-' || ch == '+' || ch == '.' || ch == 'e' || ch == 'E') {
                    pos++;
                } else {
                    break;
                }
            }
            String raw = text.substring(start, pos);
            if (raw.isEmpty()) {
                return 0;
            }
            if (raw.contains(".") || raw.contains("e") || raw.contains("E")) {
                return Double.parseDouble(raw);
            }
            return Long.parseLong(raw);
        }

        private char peek() {
            return pos < text.length() ? text.charAt(pos) : '\0';
        }

        private void expect(String literal) {
            if (!text.startsWith(literal, pos)) {
                throw new IllegalArgumentException("Некорректный JSON: ожидался " + literal);
            }
            pos += literal.length();
        }

        void skipWhitespace() {
            while (pos < text.length() && Character.isWhitespace(text.charAt(pos))) {
                pos++;
            }
        }
    }

    // ----------------------------------------------------------- сериализация

    public static String write(Object value) {
        StringBuilder builder = new StringBuilder();
        writeValue(builder, value);
        return builder.toString();
    }

    private static void writeValue(StringBuilder out, Object value) {
        if (value == null) {
            out.append("null");
        } else if (value instanceof String) {
            writeString(out, (String) value);
        } else if (value instanceof Boolean || value instanceof Number) {
            out.append(value);
        } else if (value instanceof Map) {
            Map<?, ?> map = (Map<?, ?>) value;
            out.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                writeString(out, String.valueOf(entry.getKey()));
                out.append(':');
                writeValue(out, entry.getValue());
            }
            out.append('}');
        } else if (value instanceof Iterable) {
            out.append('[');
            boolean first = true;
            for (Object item : (Iterable<?>) value) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                writeValue(out, item);
            }
            out.append(']');
        } else if (value instanceof Object[]) {
            writeValue(out, java.util.Arrays.asList((Object[]) value));
        } else {
            writeString(out, String.valueOf(value));
        }
    }

    private static void writeString(StringBuilder out, String value) {
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"': out.append("\\\""); break;
                case '\\': out.append("\\\\"); break;
                case '\n': out.append("\\n"); break;
                case '\r': out.append("\\r"); break;
                case '\t': out.append("\\t"); break;
                case '\b': out.append("\\b"); break;
                case '\f': out.append("\\f"); break;
                default:
                    if (ch < 0x20) {
                        out.append(String.format("\\u%04x", (int) ch));
                    } else {
                        out.append(ch);
                    }
            }
        }
        out.append('"');
    }

    // ------------------------------------------------------------- помощники

    public static String getString(Map<String, Object> map, String key, String fallback) {
        Object value = map.get(key);
        return value == null ? fallback : String.valueOf(value);
    }

    public static long getLong(Map<String, Object> map, String key, long fallback) {
        Object value = map.get(key);
        if (value instanceof Number) {
            return ((Number) value).longValue();
        }
        if (value instanceof String) {
            try {
                return Long.parseLong((String) value);
            } catch (NumberFormatException ignored) {
                return fallback;
            }
        }
        return fallback;
    }

    public static int getInt(Map<String, Object> map, String key, int fallback) {
        return (int) getLong(map, key, fallback);
    }

    public static boolean getBoolean(Map<String, Object> map, String key, boolean fallback) {
        Object value = map.get(key);
        if (value instanceof Boolean) {
            return ((Boolean) value).booleanValue();
        }
        if (value instanceof Number) {
            return ((Number) value).intValue() != 0;
        }
        return fallback;
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> getObject(Map<String, Object> map, String key) {
        Object value = map.get(key);
        return value instanceof Map ? (Map<String, Object>) value : null;
    }

    @SuppressWarnings("unchecked")
    public static List<Object> getArray(Map<String, Object> map, String key) {
        Object value = map.get(key);
        return value instanceof List ? (List<Object>) value : null;
    }

    @SuppressWarnings("unchecked")
    public static List<Map<String, Object>> getObjectList(Map<String, Object> map, String key) {
        List<Object> raw = getArray(map, key);
        if (raw == null) {
            return null;
        }
        List<Map<String, Object>> result = new ArrayList<>();
        for (Object item : raw) {
            if (item instanceof Map) {
                result.add((Map<String, Object>) item);
            }
        }
        return result;
    }
}
