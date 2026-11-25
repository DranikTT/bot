package bot;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.logging.Logger;
import javax.imageio.ImageIO;

/**
 * Отправка меню с баннером и без — исправляет DOCUMENT_INVALID.
 *
 * Раньше бот всегда вызывал sendPhoto() с путём к картинке. Если файла не
 * было на диске (папка image/ отсутствовала), Telegram отвечал
 * «400: DOCUMENT_INVALID», и меню вообще не отправлялось.
 *
 * Здесь три уровня защиты:
 *   1. файл проверяется на существование и читается через ImageIO (это
 *      ловит и «не картинку», и битые JPEG);
 *   2. если проверка не прошла — меню уходит текстом с рабочими кнопками;
 *   3. если Telegram всё равно отклонил фото — повторяем текстом.
 */
public final class MenuSender {

    private static final Logger LOG = Logger.getLogger(MenuSender.class.getName());

    private final TelegramApi api;
    private final Map<String, String> fileIds = new HashMap<>();
    private final Map<String, Long> validatedAt = new HashMap<>();
    private final Map<String, Path> banners = new HashMap<>();

    public MenuSender(TelegramApi api, Config config) {
        this.api = api;
        this.banners.put("main", config.ASSETS_DIR.resolve("banner_main.jpg"));
        this.banners.put("settings", config.ASSETS_DIR.resolve("banner_settings.jpg"));
        this.banners.put("subscription", config.ASSETS_DIR.resolve("banner_subscription.jpg"));
        this.banners.put("admin", config.ASSETS_DIR.resolve("banner_admin.jpg"));
    }

    /** Можно ли отправлять баннер как фото. */
    public boolean bannerIsValid(String key) {
        Path path = banners.get(key);
        if (path == null || !Files.isRegularFile(path)) {
            return false;
        }
        try {
            long modified = Files.getLastModifiedTime(path).toMillis();
            Long cached = validatedAt.get(key);
            if (cached != null && cached == modified) {
                return fileIds.containsKey(key) || isReallyImage(path);
            }
            boolean valid = isReallyImage(path);
            validatedAt.put(key, modified);
            return valid;
        } catch (IOException error) {
            return false;
        }
    }

    private static boolean isReallyImage(Path path) {
        try {
            // ImageIO вернёт null для файла, который не является картинкой
            return ImageIO.read(path.toFile()) != null;
        } catch (Exception error) {
            return false;
        }
    }

    /**
     * Показывает меню: по возможности заменяет текущее сообщение,
     * иначе отправляет новое.
     */
    public void show(long chatId, Menus.Menu menu, Long messageId, String businessConnectionId) {
        if (messageId != null) {
            if (editExisting(chatId, messageId, menu)) {
                return;
            }
        }
        sendNew(chatId, menu, businessConnectionId);
    }

    private boolean editExisting(long chatId, long messageId, Menus.Menu menu) {
        Map<String, Object> keyboard = menu.keyboard();

        try {
            api.editMessageText(chatId, messageId, menu.text(), keyboard);
            return true;
        } catch (TelegramApi.TelegramException error) {
            if (error.isNotEditable()) {
                // «there is no text in the message to edit» — под кнопками
                // старое меню с фото. Править нечего, отправляем новое.
                LOG.fine("editMessageText: сообщение не редактируется ("
                        + error.description() + "), отправляю новое");
                return false;
            }
            LOG.fine("editMessageText: " + error.getMessage());
        }

        // Меню было фото — пробуем заменить медиа, чтобы не плодить сообщения
        String banner = menu.banner();
        if (banner != null && bannerIsValid(banner)) {
            try {
                api.editMessageMedia(chatId, messageId, banners.get(banner), menu.text(), keyboard);
                return true;
            } catch (Exception error) {
                LOG.fine("editMessageMedia не сработал: " + error.getMessage());
            }
        }
        return false;
    }

    private void sendNew(long chatId, Menus.Menu menu, String businessConnectionId) {
        String banner = menu.banner();

        if (banner != null && bannerIsValid(banner)) {
            try {
                Map<String, Object> response = api.sendPhoto(chatId, banners.get(banner),
                        menu.text(), menu.keyboard());

                Object photo = response.get("photo");
                if (photo instanceof java.util.List) {
                    java.util.List<?> sizes = (java.util.List<?>) photo;
                    if (!sizes.isEmpty() && sizes.get(sizes.size() - 1) instanceof Map) {
                        Object fileId = ((Map<?, ?>) sizes.get(sizes.size() - 1)).get("file_id");
                        if (fileId != null) {
                            fileIds.put(banner, String.valueOf(fileId));
                        }
                    }
                }
                return;
            } catch (Exception error) {
                // Самый важный кусок: даже если Telegram отверг картинку,
                // меню обязано уйти — иначе пользователь видит «ничего».
                LOG.warning("[sendMenu] Баннер «" + banner + "» не отправлен: " + error.getMessage());
            }
        }

        // sendMessage сам снимет кастомные эмодзи, если у бота нет права
        api.sendMessage(chatId, menu.text(), menu.keyboard(), businessConnectionId, true);
    }

    /** Отправляет простое сообщение с клавиатурой. */
    public void sendMessage(long chatId, String text, Map<String, Object> keyboard) {
        api.sendMessage(chatId, text, keyboard, null, true);
    }
}
