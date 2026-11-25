package bot;

import java.io.IOException;
import java.io.InputStreamReader;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/** Экспорт переписки в HTML и импорт экспорта Telegram Desktop. */
public final class Archive {

    private static final DateTimeFormatter DATE =
            DateTimeFormatter.ofPattern("dd.MM.yyyy HH:mm:ss").withZone(ZoneId.systemDefault());

    private static final Map<String, String> TYPE_ALIASES = Map.of(
            "voice_message", "voice",
            "round_video", "video_note",
            "video_message", "video_note",
            "video_file", "video",
            "audio_file", "audio",
            "gif", "animation");

    private Archive() {
    }

    public static String normalizeType(String value) {
        String type = value == null ? "text" : value.toLowerCase();
        return TYPE_ALIASES.getOrDefault(type, type);
    }

    // ------------------------------------------------------------- экспорт

    public static final class ExportResult {
        private final String html;
        private final int count;

        public ExportResult(String html, int count) {
            this.html = html;
            this.count = count;
        }

        public String html() {
            return html;
        }

        public int count() {
            return count;
        }
    }

    public static ExportResult export(Database db, long ownerId, String chatId,
                                      String chatTitle, List<String> allowedTypes,
                                      long maxMediaBytes) {
        List<Map<String, Object>> rows = db.getChatMessages(chatId, ownerId);
        List<String> allowed = new ArrayList<>();
        for (String type : allowedTypes) {
            allowed.add(normalizeType(type));
        }

        StringBuilder items = new StringBuilder();
        int count = 0;

        for (Map<String, Object> row : rows) {
            String type = normalizeType(Json.getString(row, "media_type", "text"));
            if (!allowed.contains(type)) {
                continue;
            }

            boolean outgoing = Json.getLong(row, "is_outgoing", 0) == 1;
            String author = outgoing ? "Вы" : Menus.escape(chatTitle);
            String text = Json.getString(row, "text", "");

            String mediaHtml = mediaHtml(row, type, maxMediaBytes);
            boolean stickerOnly = "sticker".equals(type) && text.isBlank();

            String textHtml = text.isBlank() ? ""
                    : "<div class=\"btxt\">" + Menus.escape(text).replace("\n", "<br>") + "</div>";

            long date = Json.getLong(row, "date", 0);
            String dateText = date > 0 ? DATE.format(Instant.ofEpochSecond(date)) : "";

            items.append("<div class=\"row ").append(outgoing ? "out" : "in")
                    .append("\"><div class=\"bubble ").append(stickerOnly ? "sticker-only" : "")
                    .append("\"><div class=\"bhead\"><span class=\"author\">").append(author)
                    .append("</span><span>").append(dateText).append("</span></div>")
                    .append(textHtml).append(mediaHtml).append("</div></div>");
            count++;
        }

        String html = "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"UTF-8\">"
                + "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">"
                + "<title>Архив: " + Menus.escape(chatTitle) + "</title><style>" + CSS + "</style></head><body>"
                + "<div class=\"archive\"><div class=\"head\"><div>"
                + "<div class=\"title\">" + Menus.escape(chatTitle) + "</div>"
                + "<div class=\"info\">Идентификатор чата: " + Menus.escape(chatId) + "</div></div>"
                + "<div class=\"stats\"><div>Всего сообщений: " + count + "</div>"
                + "<div style=\"font-size:.75rem;color:var(--sub)\">Экспорт завершён</div>"
                + "</div></div><div class=\"list\">" + items + "</div></div></body></html>";

        return new ExportResult(html, count);
    }

    private static String mediaHtml(Map<String, Object> row, String type, long maxBytes) {
        String localPath = Json.getString(row, "local_path", "");
        String encoded = null;
        String mime = "application/octet-stream";

        if (!localPath.isBlank()) {
            Path path = Path.of(localPath);
            if (Files.isRegularFile(path)) {
                try {
                    if (Files.size(path) <= maxBytes) {
                        encoded = Base64.getEncoder().encodeToString(Files.readAllBytes(path));
                        mime = mimeOf(path);
                    }
                } catch (IOException ignored) {
                    // файл мог исчезнуть — покажем заглушку
                }
            }
        }

        switch (type) {
            case "photo":
                return encoded != null
                        ? "<div class=\"media photo\"><img src=\"data:" + mime + ";base64," + encoded
                        + "\" alt=\"Фото\" loading=\"lazy\"></div>"
                        : placeholder("📷 Фотография");
            case "sticker":
                return encoded != null
                        ? "<div class=\"media sticker\"><img src=\"data:" + mime + ";base64," + encoded
                        + "\" alt=\"Стикер\"></div>"
                        : placeholder("🎭 Стикер");
            case "voice":
                return encoded != null
                        ? "<div class=\"media voice\"><audio controls preload=\"none\"><source src=\"data:"
                        + mime + ";base64," + encoded + "\"></audio></div>"
                        : placeholder("🎤 Голосовое");
            case "video_note":
                return encoded != null
                        ? "<div class=\"media round\"><video src=\"data:" + mime + ";base64," + encoded
                        + "\" controls playsinline></video></div>"
                        : placeholder("🔘 Кружок");
            case "video":
                return encoded != null
                        ? "<div class=\"media video\"><video src=\"data:" + mime + ";base64," + encoded
                        + "\" controls playsinline></video></div>"
                        : placeholder("🎬 Видео");
            case "animation":
                return encoded != null
                        ? "<div class=\"media photo\"><video src=\"data:" + mime + ";base64," + encoded
                        + "\" autoplay loop muted playsinline></video></div>"
                        : placeholder("🎞 Гиф");
            case "document":
                String name = Menus.escape(Json.getString(row, "file_name", "Документ"));
                return "<div class=\"file\"><div class=\"ficon\">📁</div><div><div class=\"fname\">"
                        + name + "</div><div class=\"fsub\">Вложенный файл</div></div></div>";
            case "location":
                return placeholder("📍 Геолокация");
            default:
                return "";
        }
    }

    private static String placeholder(String text) {
        return "<div class=\"placeholder\">[" + text + "]</div>";
    }

    private static String mimeOf(Path path) {
        String name = path.getFileName().toString().toLowerCase();
        if (name.endsWith(".jpg") || name.endsWith(".jpeg")) {
            return "image/jpeg";
        }
        if (name.endsWith(".png")) {
            return "image/png";
        }
        if (name.endsWith(".webp")) {
            return "image/webp";
        }
        if (name.endsWith(".mp4")) {
            return "video/mp4";
        }
        if (name.endsWith(".webm")) {
            return "video/webm";
        }
        if (name.endsWith(".ogg") || name.endsWith(".oga")) {
            return "audio/ogg";
        }
        if (name.endsWith(".mp3")) {
            return "audio/mpeg";
        }
        return "application/octet-stream";
    }

    // -------------------------------------------------------------- импорт

    public static int importDesktop(Path jsonPath, Database db, long ownerId) throws IOException {
        String raw = Files.readString(jsonPath, StandardCharsets.UTF_8);
        Map<String, Object> data = Json.parseObject(raw);

        List<Map<String, Object>> messages = Json.getObjectList(data, "messages");
        if (messages == null) {
            throw new IllegalArgumentException(
                    "Некорректная структура result.json: нет массива messages.");
        }

        String chatId = String.valueOf(Json.getLong(data, "id",
                System.currentTimeMillis()));
        String chatTitle = Json.getString(data, "name", "Чат " + chatId);
        int count = 0;

        for (Map<String, Object> item : messages) {
            if (!"message".equals(Json.getString(item, "type", ""))) {
                continue;
            }

            String senderId = Json.getString(item, "from_id", "0").replace("user", "");
            long date = Json.getLong(item, "date_unixtime", parseDate(item));

            String mediaType = "text";
            if (item.get("photo") != null) {
                mediaType = "photo";
            } else if (item.get("media_type") != null) {
                mediaType = normalizeType(Json.getString(item, "media_type", "text"));
            } else if (item.get("file") != null) {
                mediaType = "document";
            }

            db.saveMessage(
                    chatId,
                    Json.getLong(item, "id", count + 1),
                    ownerId,
                    senderId.matches("\\d+") ? Long.parseLong(senderId) : null,
                    Json.getString(item, "from", ""),
                    "",
                    chatTitle,
                    "",
                    parseText(item.get("text")),
                    mediaType,
                    null,
                    item.get("file") instanceof String ? (String) item.get("file") : null,
                    senderId.equals(String.valueOf(ownerId)),
                    date > 0 ? date : System.currentTimeMillis() / 1000);

            count++;
        }
        return count;
    }

    private static long parseDate(Map<String, Object> item) {
        String text = Json.getString(item, "date", "");
        if (text.isEmpty()) {
            return 0;
        }
        try {
            return LocalDateTime.parse(text.replace("Z", ""))
                    .atZone(ZoneId.systemDefault()).toEpochSecond();
        } catch (Exception ignored) {
            return 0;
        }
    }

    private static String parseText(Object value) {
        if (value == null) {
            return "";
        }
        if (value instanceof String) {
            return (String) value;
        }
        if (value instanceof List) {
            StringBuilder builder = new StringBuilder();
            for (Object part : (List<?>) value) {
                if (part instanceof String) {
                    builder.append((String) part);
                } else if (part instanceof Map && ((Map<?, ?>) part).get("text") != null) {
                    builder.append(((Map<?, ?>) part).get("text"));
                }
            }
            return builder.toString();
        }
        return String.valueOf(value);
    }

    /** Распаковывает ZIP с защитой от zip-slip и возвращает путь к result.json. */
    public static Path extractZip(Path zipPath, Path destination) throws IOException {
        Files.createDirectories(destination);
        Path root = destination.toAbsolutePath().normalize();

        try (ZipInputStream zip = new ZipInputStream(Files.newInputStream(zipPath))) {
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null) {
                Path target = root.resolve(entry.getName()).normalize();
                if (!target.startsWith(root)) {
                    continue; // попытка выйти за пределы каталога
                }
                if (entry.isDirectory()) {
                    Files.createDirectories(target);
                    continue;
                }
                Files.createDirectories(target.getParent());
                Files.copy(zip, target);
            }
        }

        try (java.util.stream.Stream<Path> stream = Files.walk(root)) {
            java.util.Optional<Path> found = stream
                    .filter(path -> path.getFileName() != null
                            && "result.json".equalsIgnoreCase(path.getFileName().toString()))
                    .findFirst();
            if (found.isPresent()) {
                return found.get();
            }
            throw new IOException("В ZIP-архиве не найден result.json.");
        }
    }

    /** Читает JSON из ZIP-потока напрямую (для проверок). */
    public static Reader reader(Path path) throws IOException {
        return new InputStreamReader(Files.newInputStream(path), StandardCharsets.UTF_8);
    }

    private static final String CSS =
            ":root{--bg:#0e1621;--chat:#17212b;--in:#182533;--out:#2b5278;--text:#f5f5f5; --sub:#7f91a4;--accent:#5288c1;--border:#242f3d;--radius:14px} *{box-sizing:border-box;margin:0;padding:0} body{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,Arial,sans-serif; background:var(--bg);color:var(--text);display:flex;justify-content:center;min-height:100vh;padding:12px} .archive{width:100%;max-width:860px;background:var(--chat);border-radius:12px; overflow:hidden;display:flex;flex-direction:column;box-shadow:0 8px 30px rgba(0,0,0,.5)} .head{padding:16px 20px;border-bottom:1px solid var(--border);display:flex; justify-content:space-between;align-items:center;position:sticky;top:0;background:var(--chat);z-index:100} .title{font-size:1.15rem;font-weight:600}.info{font-size:.85rem;color:var(--sub);margin-top:2px} .stats{text-align:right;font-size:.85rem;color:var(--accent)} .list{padding:20px 16px;display:flex;flex-direction:column;gap:12px} .row{display:flex;width:100%}.row.in{justify-content:flex-start}.row.out{justify-content:flex-end} .bubble{max-width:78%;padding:10px 14px;border-radius:var(--radius);word-wrap:break-word; font-size:.95rem;line-height:1.45} .bubble.sticker-only{background:transparent!important;padding:2px!important} .row.in .bubble:not(.sticker-only){background:var(--in);border-bottom-left-radius:4px} .row.out .bubble:not(.sticker-only){background:var(--out);border-bottom-right-radius:4px} .bhead{display:flex;justify-content:space-between;gap:14px;font-size:.75rem; color:var(--sub);margin-bottom:4px} .author{font-weight:600;color:var(--accent)}.row.out .author{color:#92c5ff} .btxt{word-break:break-word} .media{margin-top:8px;border-radius:8px;overflow:hidden} .media.photo img,.media.video video{max-width:100%;max-height:480px;border-radius:8px;display:block} .media.sticker{width:170px;height:170px} .media.sticker img{width:100%;height:100%;object-fit:contain} .media.round video{width:230px;height:230px;border-radius:50%;object-fit:cover;display:block} .media.voice audio{width:100%;min-width:250px;margin-top:6px} .file{display:flex;gap:10px;align-items:center;padding:8px 10px;background:rgba(0,0,0,.15);border-radius:8px;margin-top:6px} .ficon{font-size:1.6rem}.fname{font-weight:500}.fsub{font-size:.75rem;color:var(--sub)} .placeholder{margin-top:6px;font-style:italic;color:var(--sub);font-size:.85rem}";
}
