package bot;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * Конфигурация: читается из .env и переменных окружения. Секретов в коде нет.
 */
public final class Config {

    /**
     * Корень проекта. Если бота запускают из каталога java/, поднимаемся на
     * уровень выше — там лежат assets/ и .env. Так работает и запуск из корня,
     * и запуск из подкаталога сборки.
     */
    public static final Path BASE_DIR = resolveRoot();

    private static Path resolveRoot() {
        Path cwd = Path.of("").toAbsolutePath();
        if (Files.isDirectory(cwd.resolve("assets")) || Files.isRegularFile(cwd.resolve(".env"))) {
            return cwd;
        }
        Path parent = cwd.getParent();
        if (parent != null
                && (Files.isDirectory(parent.resolve("assets")) || Files.isRegularFile(parent.resolve(".env")))) {
            return parent;
        }
        return cwd;
    }
    public static final Path ASSETS_DIR = BASE_DIR.resolve("assets");
    public static final Path MEDIA_DIR = BASE_DIR.resolve("media_cache");
    public static final Path TEMP_DIR = BASE_DIR.resolve("temp");

    public final String botToken;
    public final List<Long> adminIds;
    public final Path databasePath;
    public final String encryptionKey;
    public final String apiBase;
    public final long maxImportFileMb;
    public final long maxImportedMediaMb;
    public final long maxSingleMediaMb;
    public final long maxExportMb;

    private Config(Env env) {
        this.botToken = env.require("BOT_TOKEN");
        this.adminIds = Collections.unmodifiableList(parseIds(env.get("ADMIN_IDS", "")));
        this.databasePath = Path.of(env.get("DATABASE_PATH", BASE_DIR.resolve("bot_database.sqlite").toString()));
        this.encryptionKey = env.get("DB_ENCRYPTION_KEY", "SaveMod_AES256_Default_Secret_Key_2025_Secure!");
        this.apiBase = env.get("API_SERVER", "https://api.telegram.org");
        this.maxExportMb = env.getLong("MAX_EXPORT_MB", 48);
        this.maxSingleMediaMb = env.getLong("MAX_SINGLE_MEDIA_MB", 35);
        this.maxImportFileMb = env.getLong("MAX_IMPORT_FILE_MB", 200);
        this.maxImportedMediaMb = env.getLong("MAX_IMPORTED_MEDIA_MB", 100);
    }

    public static Config load() {
        Env env = Env.load(BASE_DIR.resolve(".env"));
        try {
            Files.createDirectories(MEDIA_DIR);
            Files.createDirectories(TEMP_DIR);
        } catch (IOException error) {
            throw new IllegalStateException("Не удалось создать рабочие каталоги: " + error.getMessage(), error);
        }
        return new Config(env);
    }

    public boolean isAdmin(long userId) {
        return adminIds.contains(userId);
    }

    private static List<Long> parseIds(String raw) {
        List<Long> ids = new ArrayList<>();
        for (String part : raw.split("[,\\s]+")) {
            String trimmed = part.trim();
            if (trimmed.isEmpty()) {
                continue;
            }
            try {
                long value = Long.parseLong(trimmed);
                if (value > 0) {
                    ids.add(value);
                }
            } catch (NumberFormatException ignored) {
                // некорректный ID просто пропускаем
            }
        }
        return ids;
    }

    /** Простое чтение .env без сторонних библиотек. */
    private static final class Env {
        private final java.util.Map<String, String> values = new java.util.HashMap<>();

        static Env load(Path envFile) {
            Env env = new Env();
            if (Files.isRegularFile(envFile)) {
                try {
                    for (String rawLine : Files.readAllLines(envFile, StandardCharsets.UTF_8)) {
                        String line = rawLine.trim();
                        if (line.isEmpty() || line.startsWith("#") || !line.contains("=")) {
                            continue;
                        }
                        int index = line.indexOf('=');
                        String key = line.substring(0, index).trim();
                        String value = line.substring(index + 1).trim();
                        if (value.length() >= 2
                                && (value.startsWith("\"") && value.endsWith("\"")
                                || value.startsWith("'") && value.endsWith("'"))) {
                            value = value.substring(1, value.length() - 1);
                        }
                        if (!key.isEmpty()) {
                            env.values.put(key, value);
                        }
                    }
                } catch (IOException error) {
                    throw new IllegalStateException("Не удалось прочитать .env: " + error.getMessage(), error);
                }
            }
            return env;
        }

        String get(String key, String fallback) {
            String fromEnv = System.getenv(key);
            if (fromEnv != null && !fromEnv.isBlank()) {
                return fromEnv.trim();
            }
            return values.getOrDefault(key, fallback);
        }

        String require(String key) {
            String value = get(key, "").trim();
            if (value.isEmpty()) {
                throw new IllegalStateException(
                        "Не задан " + key + ". Добавьте его в .env или в переменные окружения.");
            }
            return value;
        }

        long getLong(String key, long fallback) {
            try {
                long value = Long.parseLong(get(key, "").trim());
                return value > 0 ? value : fallback;
            } catch (NumberFormatException ignored) {
                return fallback;
            }
        }
    }
}
