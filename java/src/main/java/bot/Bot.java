package bot;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.logging.Level;
import java.util.logging.Logger;

/** Точка входа: long polling без вебхуков и внешних зависимостей. */
public final class Bot {

    private static final Logger LOG = Logger.getLogger(Bot.class.getName());

    private static final List<String> ALLOWED_UPDATES = List.of(
            "message",
            "callback_query",
            "inline_query",
            "pre_checkout_query",
            "business_connection",
            "business_message",
            "edited_business_message",
            "deleted_business_messages");

    private final Config config;
    private final Database db;
    private final TelegramApi api;
    private final MenuSender sender;
    private final UpdateHandler handler;

    private Bot(Config config) {
        this.config = config;
        this.db = new Database(config.databasePath, config.encryptionKey);
        this.api = new TelegramApi(config.botToken, config.apiBase);
        this.sender = new MenuSender(api, config);
        this.handler = new UpdateHandler(api, db, config, sender);
    }

    public static void main(String[] args) {
        System.setProperty("java.util.logging.SimpleFormatter.format",
                "%1$tF %1$tT %4$s %5$s%6$s%n");

        Config config;
        try {
            config = Config.load();
        } catch (Exception error) {
            LOG.severe("Не удалось прочитать конфигурацию: " + error.getMessage());
            return;
        }

        try (Database db = new Database(config.databasePath, config.encryptionKey)) {
            TelegramApi api = new TelegramApi(config.botToken, config.apiBase);
            MenuSender sender = new MenuSender(api, config);
            UpdateHandler handler = new UpdateHandler(api, db, config, sender);

            Map<String, Object> me = api.getMe();
            String username = Json.getString(me, "username", "");
            handler.setBotUsername(username);

            LOG.info("Бот запущен: @" + username + " (long polling)");
            api.deleteWebhook();
            registerCommands(api);

            long offset = 0;
            while (true) {
                try {
                    List<Map<String, Object>> updates =
                            api.getUpdates(offset, 30, ALLOWED_UPDATES);

                    for (Map<String, Object> update : updates) {
                        offset = Math.max(offset, Json.getLong(update, "update_id", 0) + 1);
                        handler.handle(update);
                    }
                } catch (TelegramApi.TelegramException error) {
                    if (error.code() == 401 || error.code() == 404) {
                        LOG.severe("Неверный токен бота: " + error.description());
                        return;
                    }
                    LOG.log(Level.WARNING, "Опрос обновлений: " + error.getMessage());
                    sleep(2000);
                } catch (Exception error) {
                    LOG.log(Level.WARNING, "Сбой цикла", error);
                    sleep(3000);
                }
            }
        } catch (Exception error) {
            LOG.log(Level.SEVERE, "Запуск не удался", error);
        }
    }

    private static void registerCommands(TelegramApi api) {
        List<Map<String, Object>> commands = new ArrayList<>();
        commands.add(Map.of("command", "start", "description", "Главное меню"));
        commands.add(Map.of("command", "menu", "description", "Показать меню"));
        commands.add(Map.of("command", "export", "description", "Экспорт переписки в HTML"));
        commands.add(Map.of("command", "archive", "description", "Импорт архива Telegram"));
        commands.add(Map.of("command", "connect", "description", "Подключение к Business"));
        try {
            api.setMyCommands(commands);
        } catch (Exception error) {
            LOG.fine("setMyCommands: " + error.getMessage());
        }
    }

    private static void sleep(long millis) {
        try {
            Thread.sleep(millis);
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
        }
    }
}
