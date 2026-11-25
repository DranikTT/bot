package bot;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;

import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Локальный двойник Bot API для оффлайн-тестов.
 *
 * Отвечает на все методы, которые использует бот, записывает вызовы и
 * раздаёт заранее подготовленные апдейты через getUpdates.
 */
public final class FakeTelegram {

    private final HttpServer server;
    private final Map<String, List<Map<String, Object>>> calls = new ConcurrentHashMap<>();
    private final List<Map<String, Object>> queue = Collections.synchronizedList(new ArrayList<>());
    private final AtomicLong messageCounter = new AtomicLong(1000);
    private final List<String> errors = Collections.synchronizedList(new ArrayList<>());
    /** Хронологический журнал: метод + payload, в порядке поступления. */
    private final List<Map.Entry<String, Map<String, Object>>> timeline =
            Collections.synchronizedList(new ArrayList<>());

    public FakeTelegram(int port) throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", port), 0);
        server.createContext("/", this::handle);
        server.setExecutor(java.util.concurrent.Executors.newFixedThreadPool(4));
    }

    public String baseUrl() {
        return "http://127.0.0.1:" + server.getAddress().getPort();
    }

    public void start() {
        server.start();
    }

    public void stop() {
        server.stop(0);
    }

    public void pushUpdate(Map<String, Object> update) {
        queue.add(update);
    }

    public List<Map<String, Object>> callsOf(String method) {
        return calls.getOrDefault(method, new ArrayList<>());
    }

    public List<String> errors() {
        return errors;
    }

    public int totalCalls() {
        int total = 0;
        for (List<Map<String, Object>> list : calls.values()) {
            total += list.size();
        }
        return total;
    }

    public void reset() {
        calls.clear();
        errors.clear();
        timeline.clear();
    }

    /** Хронологический журнал вызовов. */
    public List<Map.Entry<String, Map<String, Object>>> timeline() {
        return timeline;
    }

    /** Текст последнего sendMessage/sendPhoto (по реальному порядку). */
    public String lastText() {
        synchronized (timeline) {
            for (int i = timeline.size() - 1; i >= 0; i--) {
                Map.Entry<String, Map<String, Object>> entry = timeline.get(i);
                String method = entry.getKey();
                if ("sendMessage".equals(method)) {
                    String text = Json.getString(entry.getValue(), "text", "");
                    if (!text.isEmpty()) {
                        return text;
                    }
                } else if ("sendPhoto".equals(method)) {
                    String caption = Json.getString(entry.getValue(), "caption", "");
                    if (!caption.isEmpty()) {
                        return caption;
                    }
                }
            }
        }
        return "";
    }

    // ------------------------------------------------------------- обработка

    private void handle(HttpExchange exchange) throws IOException {
        try {
            String path = exchange.getRequestURI().getPath();
            String method = path.substring(path.lastIndexOf('/') + 1);

            byte[] raw = exchange.getRequestBody().readAllBytes();
            String body = new String(raw, StandardCharsets.UTF_8);

            Map<String, Object> payload = "GET".equals(exchange.getRequestMethod())
                    ? new LinkedHashMap<>()
                    : parsePayload(body, exchange.getRequestHeaders().getFirst("Content-Type"));

            calls.computeIfAbsent(method, key -> Collections.synchronizedList(new ArrayList<>()))
                    .add(payload);
            timeline.add(new java.util.AbstractMap.SimpleEntry<>(method, payload));

            Object result = switchMethod(method, payload);
            Map<String, Object> response = new LinkedHashMap<>();
            response.put("ok", true);
            response.put("result", result);
            respond(exchange, 200, Json.write(response));

        } catch (Throwable error) {
            errors.add(error.toString());
            Map<String, Object> response = new LinkedHashMap<>();
            response.put("ok", false);
            response.put("error_code", 400);
            response.put("description", String.valueOf(error.getMessage()));
            respond(exchange, 400, Json.write(response));
        }
    }

    private Object switchMethod(String method, Map<String, Object> payload) {
        switch (method) {
            case "getMe":
                return Map.of("id", 42L, "is_bot", true, "first_name", "Fake",
                        "username", "fake_bot");
            case "getUpdates": {
                List<Object> result = new ArrayList<>();
                synchronized (queue) {
                    long offset = Json.getLong(payload, "offset", 0);
                    var iterator = queue.iterator();
                    while (iterator.hasNext()) {
                        Map<String, Object> update = iterator.next();
                        if (Json.getLong(update, "update_id", 0) >= offset) {
                            result.add(update);
                        }
                    }
                }
                return result;
            }
            case "sendMessage":
            case "sendPhoto":
            case "sendInvoice":
            case "sendDocument":
            case "copyMessage": {
                long messageId = messageCounter.incrementAndGet();
                Map<String, Object> message = new LinkedHashMap<>();
                message.put("message_id", messageId);
                message.put("chat", Map.of("id", Json.getLong(payload, "chat_id", 0), "type", "private"));
                message.put("date", System.currentTimeMillis() / 1000);
                message.put("text", Json.getString(payload, "text", ""));

                // Telegram отдаёт размеры фото — бот кэширует file_id
                message.put("photo", List.of(Map.of("file_id", "file_" + messageId,
                        "file_unique_id", "u" + messageId, "width", 1280, "height", 640)));

                Object replyMarkup = payload.get("reply_markup");
                if (replyMarkup != null) {
                    message.put("reply_markup", replyMarkup);
                }
                return message;
            }
            case "editMessageText":
            case "editMessageMedia":
                return Map.of("message_id", Json.getLong(payload, "message_id", 0),
                        "chat", Map.of("id", Json.getLong(payload, "chat_id", 0), "type", "private"),
                        "date", System.currentTimeMillis() / 1000);
            case "answerCallbackQuery":
            case "answerPreCheckoutQuery":
            case "deleteMessage":
            case "deleteWebhook":
            case "setMyCommands":
                return true;
            case "answerInlineQuery":
                return true;
            case "getFile":
                return Map.of("file_id", Json.getString(payload, "file_id", ""),
                        "file_path", "documents/result.json");
            default:
                return true;
        }
    }

    private static Map<String, Object> parsePayload(String body, String contentType) {
        if (contentType != null && contentType.startsWith("multipart/form-data")) {
            return parseMultipart(body, contentType);
        }
        if (body == null || body.isBlank()) {
            return new LinkedHashMap<>();
        }
        return Json.parseObject(body);
    }

    private static Map<String, Object> parseMultipart(String body, String contentType) {
        Map<String, Object> fields = new LinkedHashMap<>();
        String boundary = contentType.substring(contentType.indexOf("boundary=") + 9).trim();
        String[] parts = body.split("--" + boundary);

        for (String part : parts) {
            int nameIndex = part.indexOf("name=\"");
            if (nameIndex < 0) {
                continue;
            }
            int nameEnd = part.indexOf('"', nameIndex + 6);
            String name = part.substring(nameIndex + 6, nameEnd);

            int split = part.indexOf("\r\n\r\n");
            if (split < 0) {
                continue;
            }
            String value = part.substring(split + 4);
            if (value.endsWith("\r\n")) {
                value = value.substring(0, value.length() - 2);
            }
            fields.putIfAbsent(name, value);
        }
        return fields;
    }

    private static void respond(HttpExchange exchange, int status, String body) throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json; charset=utf-8");
        exchange.sendResponseHeaders(status, bytes.length);
        try (OutputStream out = exchange.getResponseBody()) {
            out.write(bytes);
        }
    }
}
