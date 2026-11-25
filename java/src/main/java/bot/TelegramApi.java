package bot;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.logging.Level;
import java.util.logging.Logger;

/**
 * Клиент Bot API поверх java.net.http (Java 11+).
 *
 * Реализует только то, что нужно боту, но именно через сырой HTTP —
 * благодаря этому доступны поля, которые отсутствуют в обёртках:
 * style и icon_custom_emoji_id у inline-кнопок.
 */
public final class TelegramApi {

    private static final Logger LOG = Logger.getLogger(TelegramApi.class.getName());

    private final String token;
    private final String baseUrl;
    private final HttpClient client;

    public TelegramApi(String token, String apiBase) {
        this.token = token;
        this.baseUrl = apiBase.endsWith("/") ? apiBase.substring(0, apiBase.length() - 1) : apiBase;
        this.client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(20))
                .version(HttpClient.Version.HTTP_1_1)
                .build();
    }

    // ------------------------------------------------------------ низкий уровень

    public Map<String, Object> callJson(String method, Map<String, Object> payload) {
        Object result = callRaw(method, payload);
        if (result instanceof Map) {
            @SuppressWarnings("unchecked")
            Map<String, Object> typed = (Map<String, Object>) result;
            return typed;
        }
        return new LinkedHashMap<>();
    }

    /** Возвращает result как есть: объект, массив или примитив. */
    public Object callRaw(String method, Map<String, Object> payload) {
        String body = Json.write(payload);
        HttpRequest request = HttpRequest.newBuilder(URI.create(baseUrl + "/bot" + token + "/" + method))
                .header("Content-Type", "application/json; charset=utf-8")
                .timeout(Duration.ofSeconds(60))
                .POST(HttpRequest.BodyPublishers.ofString(body, StandardCharsets.UTF_8))
                .build();
        return executeRaw(method, request);
    }

    public Map<String, Object> callMultipart(String method, Map<String, Object> fields, String fileField, Path file)
            throws IOException {
        String boundary = "----bot" + UUID.randomUUID().toString().replace("-", "");
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();

        for (Map.Entry<String, Object> entry : fields.entrySet()) {
            Object value = entry.getValue();
            if (value == null) {
                continue;
            }
            String text = value instanceof String ? (String) value : Json.write(value);
            buffer.write(("--" + boundary + "\r\n").getBytes(StandardCharsets.UTF_8));
            buffer.write(("Content-Disposition: form-data; name=\"" + entry.getKey() + "\"\r\n\r\n")
                    .getBytes(StandardCharsets.UTF_8));
            buffer.write(text.getBytes(StandardCharsets.UTF_8));
            buffer.write("\r\n".getBytes(StandardCharsets.UTF_8));
        }

        if (file != null && Files.isRegularFile(file)) {
            buffer.write(("--" + boundary + "\r\n").getBytes(StandardCharsets.UTF_8));
            buffer.write(("Content-Disposition: form-data; name=\"" + fileField
                    + "\"; filename=\"" + file.getFileName() + "\"\r\n").getBytes(StandardCharsets.UTF_8));
            buffer.write("Content-Type: application/octet-stream\r\n\r\n".getBytes(StandardCharsets.UTF_8));
            buffer.write(Files.readAllBytes(file));
            buffer.write("\r\n".getBytes(StandardCharsets.UTF_8));
        }

        buffer.write(("--" + boundary + "--\r\n").getBytes(StandardCharsets.UTF_8));

        HttpRequest request = HttpRequest.newBuilder(URI.create(baseUrl + "/bot" + token + "/" + method))
                .header("Content-Type", "multipart/form-data; boundary=" + boundary)
                .timeout(Duration.ofSeconds(120))
                .POST(HttpRequest.BodyPublishers.ofByteArray(buffer.toByteArray()))
                .build();

        return execute(method, request);
    }

    private Map<String, Object> execute(String method, HttpRequest request) {
        Object result = executeRaw(method, request);
        if (result instanceof Map) {
            @SuppressWarnings("unchecked")
            Map<String, Object> typed = (Map<String, Object>) result;
            return typed;
        }
        return new LinkedHashMap<>();
    }

    private Object executeRaw(String method, HttpRequest request) {
        for (int attempt = 1; attempt <= 3; attempt++) {
            try {
                HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8));
                Map<String, Object> parsed = Json.parseObject(response.body());

                if (Json.getBoolean(parsed, "ok", false)) {
                    return parsed.get("result");
                }

                String description = Json.getString(parsed, "description", "неизвестная ошибка");
                int code = Json.getInt(parsed, "error_code", response.statusCode());

                LOG.warning("[" + method + "] ошибка " + code + ": " + description);
                throw new TelegramException(code, description);

            } catch (TelegramException error) {
                if (error.isRetryAfter() && attempt < 3) {
                    sleep(error.retryAfterSeconds() * 1000L);
                    continue;
                }
                throw error;
            } catch (IOException | InterruptedException error) {
                if (error instanceof InterruptedException) {
                    Thread.currentThread().interrupt();
                }
                if (attempt == 3) {
                    throw new TelegramException(0, method + ": " + error);
                }
                LOG.log(Level.FINE, "[" + method + "] сеть, попытка " + attempt, error);
                sleep(800L * attempt);
            }
        }
        throw new TelegramException(0, method + ": не удалось выполнить запрос");
    }

    private static void sleep(long millis) {
        try {
            Thread.sleep(millis);
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
        }
    }

    public String downloadUrl(String filePath) {
        return baseUrl + "/file/bot" + token + "/" + encodePath(filePath);
    }

    private static String encodePath(String path) {
        StringBuilder builder = new StringBuilder();
        for (String part : path.split("/")) {
            if (builder.length() > 0) {
                builder.append('/');
            }
            builder.append(URLEncoder.encode(part, StandardCharsets.UTF_8).replace("+", "%20"));
        }
        return builder.toString();
    }

    public byte[] download(String filePath) throws IOException {
        HttpRequest request = HttpRequest.newBuilder(URI.create(downloadUrl(filePath)))
                .timeout(Duration.ofSeconds(120))
                .GET()
                .build();
        try {
            HttpResponse<byte[]> response = client.send(request, HttpResponse.BodyHandlers.ofByteArray());
            if (response.statusCode() != 200) {
                throw new IOException("HTTP " + response.statusCode());
            }
            return response.body();
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new IOException("Загрузка прервана", error);
        }
    }

    // ------------------------------------------------------------- высокий уровень

    public Map<String, Object> getMe() {
        return callJson("getMe", new LinkedHashMap<>());
    }

    public void deleteWebhook() {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("drop_pending_updates", false);
        callJson("deleteWebhook", payload);
    }

    public void setMyCommands(List<Map<String, Object>> commands) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("commands", commands);
        callJson("setMyCommands", payload);
    }

    public Map<String, Object> sendMessage(long chatId, String text, Object keyboard,
                                           String businessConnectionId, boolean noPreview) {
        return sendMessageSafe(chatId, text, keyboard, businessConnectionId, noPreview);
    }

    /**
     * Отправка с автоматическим фолбэком на обычные эмодзи.
     *
     * Если у бота нет права на кастомные эмодзи, Telegram отвечает
     * {@code 400: DOCUMENT_INVALID} и отклоняет сообщение целиком.
     * Раньше бот в этом случае просто падал, и пользователь не видел меню.
     * Теперь тег {@code <tg-emoji>} снимается, иконки кнопок убираются,
     * и сообщение уходит со второго раза — уже с обычными эмодзи.
     */
    public Map<String, Object> sendMessageSafe(long chatId, String text, Object keyboard,
                                               String businessConnectionId, boolean noPreview) {
        try {
            return sendMessageOnce(chatId, text, keyboard, businessConnectionId, noPreview, true);
        } catch (TelegramException error) {
            if (!isCustomEmojiError(error)) {
                throw error;
            }
            Emoji.markNotAllowed();
            LOG.warning("[sendMessage] Кастомные эмодзи недоступны ("
                    + error.description() + ") — отправляю с обычными");
            return sendMessageOnce(chatId, text, keyboard, businessConnectionId, noPreview, false);
        }
    }

    private Map<String, Object> sendMessageOnce(long chatId, String text, Object keyboard,
                                                String businessConnectionId, boolean noPreview,
                                                boolean withEmoji) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("chat_id", chatId);
        payload.put("text", withEmoji ? text : Emoji.stripHtml(text));
        payload.put("parse_mode", "HTML");
        if (noPreview) {
            payload.put("link_preview_options", Map.of("is_disabled", true));
        }
        Object markup = withEmoji ? keyboard : Keyboard.stripEmojis(keyboard);
        if (markup != null) {
            payload.put("reply_markup", markup);
        }
        if (businessConnectionId != null) {
            payload.put("business_connection_id", businessConnectionId);
        }
        return callJson("sendMessage", payload);
    }

    /** Это ошибка из-за кастомных эмодзи (нет права / неизвестный ID)? */
    static boolean isCustomEmojiError(TelegramException error) {
        String text = error.description() == null ? "" : error.description().toUpperCase();
        return error.code() == 400
                && (text.contains("DOCUMENT_INVALID")
                || text.contains("CUSTOM_EMOJI")
                || text.contains("BUTTON_ICON"));
    }

    public Map<String, Object> editMessageText(long chatId, long messageId, String text, Object keyboard) {
        try {
            return editMessageTextOnce(chatId, messageId, text, keyboard, true);
        } catch (TelegramException error) {
            if (!isCustomEmojiError(error)) {
                throw error;
            }
            Emoji.markNotAllowed();
            LOG.warning("[editMessageText] Кастомные эмодзи недоступны — правлю с обычными");
            return editMessageTextOnce(chatId, messageId, text, keyboard, false);
        }
    }

    private Map<String, Object> editMessageTextOnce(long chatId, long messageId, String text,
                                                    Object keyboard, boolean withEmoji) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("chat_id", chatId);
        payload.put("message_id", messageId);
        payload.put("text", withEmoji ? text : Emoji.stripHtml(text));
        payload.put("parse_mode", "HTML");
        payload.put("link_preview_options", Map.of("is_disabled", true));
        Object markup = withEmoji ? keyboard : Keyboard.stripEmojis(keyboard);
        if (markup != null) {
            payload.put("reply_markup", markup);
        }
        return callJson("editMessageText", payload);
    }

    public Map<String, Object> sendPhoto(long chatId, Path photo, String caption, Object keyboard)
            throws IOException {
        try {
            return sendPhotoOnce(chatId, photo, caption, keyboard, true);
        } catch (TelegramException | IOException error) {
            TelegramException telegramError = asTelegramError(error);
            if (telegramError == null || !isCustomEmojiError(telegramError)) {
                throw error;
            }
            Emoji.markNotAllowed();
            LOG.warning("[sendPhoto] Кастомные эмодзи недоступны — подпись с обычными");
            return sendPhotoOnce(chatId, photo, caption, keyboard, false);
        }
    }

    private Map<String, Object> sendPhotoOnce(long chatId, Path photo, String caption,
                                              Object keyboard, boolean withEmoji) throws IOException {
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("chat_id", chatId);
        fields.put("caption", withEmoji ? caption : Emoji.stripHtml(caption));
        fields.put("parse_mode", "HTML");
        Object markup = withEmoji ? keyboard : Keyboard.stripEmojis(keyboard);
        if (markup != null) {
            fields.put("reply_markup", markup);
        }
        return callMultipart("sendPhoto", fields, "photo", photo);
    }

    private static TelegramException asTelegramError(Throwable error) {
        if (error instanceof TelegramException) {
            return (TelegramException) error;
        }
        Throwable cause = error.getCause();
        return cause instanceof TelegramException ? (TelegramException) cause : null;
    }

    public Map<String, Object> editMessageMedia(long chatId, long messageId, Path photo,
                                                String caption, Object keyboard) throws IOException {
        try {
            return editMessageMediaOnce(chatId, messageId, photo, caption, keyboard, true);
        } catch (TelegramException | IOException error) {
            TelegramException telegramError = asTelegramError(error);
            if (telegramError == null || !isCustomEmojiError(telegramError)) {
                throw error;
            }
            Emoji.markNotAllowed();
            LOG.warning("[editMessageMedia] Кастомные эмодзи недоступны — правлю с обычными");
            return editMessageMediaOnce(chatId, messageId, photo, caption, keyboard, false);
        }
    }

    private Map<String, Object> editMessageMediaOnce(long chatId, long messageId, Path photo,
                                                     String caption, Object keyboard,
                                                     boolean withEmoji) throws IOException {
        Map<String, Object> media = new LinkedHashMap<>();
        media.put("type", "photo");
        media.put("media", "attach://photo");
        media.put("caption", withEmoji ? caption : Emoji.stripHtml(caption));
        media.put("parse_mode", "HTML");

        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("chat_id", chatId);
        fields.put("message_id", messageId);
        fields.put("media", media);
        Object markup = withEmoji ? keyboard : Keyboard.stripEmojis(keyboard);
        if (markup != null) {
            fields.put("reply_markup", markup);
        }
        return callMultipart("editMessageMedia", fields, "photo", photo);
    }

    public Map<String, Object> sendDocument(long chatId, String fileName, byte[] content,
                                            String caption, Object keyboard) throws IOException {
        Path temp = Files.createTempFile("doc", ".bin");
        try {
            Files.write(temp, content);
            try {
                return sendDocumentOnce(chatId, temp, caption, keyboard, true);
            } catch (TelegramException | IOException error) {
                TelegramException telegramError = asTelegramError(error);
                if (telegramError == null || !isCustomEmojiError(telegramError)) {
                    throw error;
                }
                Emoji.markNotAllowed();
                LOG.warning("[sendDocument] Кастомные эмодзи недоступны — подпись с обычными");
                return sendDocumentOnce(chatId, temp, caption, keyboard, false);
            }
        } finally {
            Files.deleteIfExists(temp);
        }
    }

    private Map<String, Object> sendDocumentOnce(long chatId, Path temp, String caption,
                                                 Object keyboard, boolean withEmoji) throws IOException {
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("chat_id", chatId);
        fields.put("caption", withEmoji ? caption : Emoji.stripHtml(caption));
        fields.put("parse_mode", "HTML");
        Object markup = withEmoji ? keyboard : Keyboard.stripEmojis(keyboard);
        if (markup != null) {
            fields.put("reply_markup", markup);
        }
        return callMultipart("sendDocument", fields, "document", temp);
    }

    public void answerCallbackQuery(String callbackId, String text, boolean alert) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("callback_query_id", callbackId);
        if (text != null && !text.isEmpty()) {
            payload.put("text", text);
            payload.put("show_alert", alert);
        }
        try {
            callJson("answerCallbackQuery", payload);
        } catch (TelegramException error) {
            // «query is too old» — не критично
            LOG.fine("answerCallbackQuery: " + error.getMessage());
        }
    }

    public void deleteMessage(long chatId, long messageId) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("chat_id", chatId);
        payload.put("message_id", messageId);
        try {
            callJson("deleteMessage", payload);
        } catch (TelegramException ignored) {
            // сообщение уже удалено
        }
    }

    public void answerPreCheckoutQuery(String queryId) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("pre_checkout_query_id", queryId);
        payload.put("ok", true);
        callJson("answerPreCheckoutQuery", payload);
    }

    public Map<String, Object> sendInvoice(long chatId, String title, String description,
                                           String payload, int amount) {
        Map<String, Object> invoice = new LinkedHashMap<>();
        invoice.put("chat_id", chatId);
        invoice.put("title", title);
        invoice.put("description", description);
        invoice.put("payload", payload);
        invoice.put("provider_token", "");
        invoice.put("currency", "XTR");
        invoice.put("prices", List.of(Map.of("label", title, "amount", amount)));
        return callJson("sendInvoice", invoice);
    }

    public void answerInlineQuery(String queryId, List<Map<String, Object>> results) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("inline_query_id", queryId);
        payload.put("results", results);
        payload.put("cache_time", 5);
        payload.put("is_personal", true);
        callJson("answerInlineQuery", payload);
    }

    public Map<String, Object> getFile(String fileId) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("file_id", fileId);
        return callJson("getFile", payload);
    }

    public Map<String, Object> copyMessage(long chatId, long fromChatId, long messageId) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("chat_id", chatId);
        payload.put("from_chat_id", fromChatId);
        payload.put("message_id", messageId);
        return callJson("copyMessage", payload);
    }

    /** Long polling: возвращает обновления, блокируется до 30 секунд. */
    public List<Map<String, Object>> getUpdates(long offset, int timeoutSeconds, List<String> allowedUpdates) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("offset", offset);
        payload.put("timeout", timeoutSeconds);
        payload.put("allowed_updates", new ArrayList<>(allowedUpdates));
        // getUpdates возвращает массив, поэтому используем callRaw
        Object raw = callRaw("getUpdates", payload);
        List<Map<String, Object>> updates = new ArrayList<>();
        if (raw instanceof List) {
            for (Object item : (List<?>) raw) {
                if (item instanceof Map) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> map = (Map<String, Object>) item;
                    updates.add(map);
                }
            }
        }
        return updates;
    }

    /** Исключение с кодом ошибки Telegram. */
    public static final class TelegramException extends RuntimeException {
        private final int code;
        private final String description;

        TelegramException(int code, String description) {
            super(code + ": " + description);
            this.code = code;
            this.description = description;
        }

        public int code() {
            return code;
        }

        public String description() {
            return description;
        }

        public boolean isRetryAfter() {
            return code == 429;
        }

        public long retryAfterSeconds() {
            java.util.regex.Matcher matcher =
                    java.util.regex.Pattern.compile("retry after (\\d+)").matcher(description);
            return matcher.find() ? Long.parseLong(matcher.group(1)) : 1;
        }

        public boolean isMessageNotModified() {
            return description != null && description.toLowerCase().contains("message is not modified");
        }

        public boolean isMessageToEditNotFound() {
            String text = description == null ? "" : description.toLowerCase();
            return text.contains("message to edit not found")
                    || text.contains("message can't be edited");
        }

        /**
         * Сообщение не текст (например, старое меню с баннером) либо его
         * уже нет — значит, редактировать нельзя, нужно отправить новое.
         */
        public boolean isNotEditable() {
            String text = description == null ? "" : description.toLowerCase();
            return text.contains("there is no text in the message to edit")
                    || text.contains("message can't be edited")
                    || text.contains("message to edit not found")
                    || text.contains("message is not modified");
        }
    }
}
