package bot;

import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Base64;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import javax.crypto.Cipher;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;

/**
 * SQLite-хранилище. Схема и формат шифрования совпадают с прежними версиями
 * (AES-256-GCM, префикс {@code enc:v1:}), поэтому существующая база читается
 * без миграции.
 */
public final class Database implements AutoCloseable {

    private static final String ENC_PREFIX = "enc:v1:";
    private static final int GCM_TAG_BITS = 128;
    private static final int NONCE_BYTES = 12;

    private final Connection connection;
    private final SecretKeySpec key;
    private final SecureRandom random = new SecureRandom();

    public Database(Path path, String encryptionKey) {
        try {
            Class.forName("org.sqlite.JDBC");
            this.connection = DriverManager.getConnection("jdbc:sqlite:" + path.toAbsolutePath());
            try (Statement statement = connection.createStatement()) {
                statement.execute("PRAGMA journal_mode = WAL");
                statement.execute("PRAGMA synchronous = NORMAL");
                statement.execute("PRAGMA busy_timeout = 5000");
            }
            this.key = new SecretKeySpec(
                    MessageDigest.getInstance("SHA-256").digest(encryptionKey.getBytes(StandardCharsets.UTF_8)),
                    "AES");
            initSchema();
        } catch (Exception error) {
            throw new IllegalStateException("Не удалось открыть базу: " + error.getMessage(), error);
        }
    }

    // -------------------------------------------------------------- схема

    private void initSchema() throws SQLException {
        try (Statement statement = connection.createStatement()) {
            statement.executeUpdate("CREATE TABLE IF NOT EXISTS users ( user_id INTEGER PRIMARY KEY, username TEXT DEFAULT '', first_name TEXT DEFAULT '', subscription_until INTEGER DEFAULT 0, is_premium INTEGER DEFAULT 0, trial_used INTEGER DEFAULT 0, notify_deleted INTEGER DEFAULT 1, notify_edited INTEGER DEFAULT 1, referred_by INTEGER DEFAULT NULL, referrals_count INTEGER DEFAULT 0, is_banned INTEGER DEFAULT 0, ban_reason TEXT DEFAULT '', created_at INTEGER DEFAULT 0 )");
            statement.executeUpdate("CREATE TABLE IF NOT EXISTS business_connections ( connection_id TEXT PRIMARY KEY, user_id INTEGER, user_chat_id INTEGER, can_reply INTEGER DEFAULT 0, is_enabled INTEGER DEFAULT 1, updated_at INTEGER DEFAULT 0 )");
            statement.executeUpdate("CREATE TABLE IF NOT EXISTS messages ( chat_id TEXT, message_id INTEGER, connection_id TEXT DEFAULT '', owner_id INTEGER, sender_id INTEGER, sender_name TEXT DEFAULT '', sender_username TEXT DEFAULT '', peer_name TEXT DEFAULT '', peer_username TEXT DEFAULT '', text TEXT DEFAULT '', edit_history TEXT DEFAULT '[]', reactions_history TEXT DEFAULT '[]', gift_json TEXT DEFAULT '', media_type TEXT DEFAULT NULL, file_id TEXT DEFAULT NULL, file_name TEXT DEFAULT NULL, local_path TEXT DEFAULT NULL, is_self_destruct INTEGER DEFAULT 0, is_deleted INTEGER DEFAULT 0, is_outgoing INTEGER DEFAULT 0, date INTEGER DEFAULT 0, PRIMARY KEY (chat_id, message_id) )");
            statement.executeUpdate("CREATE INDEX IF NOT EXISTS idx_msg_owner ON messages(owner_id, chat_id, date)");
            statement.executeUpdate("CREATE INDEX IF NOT EXISTS idx_bconn ON business_connections(user_id, is_enabled)");
        }
        ensureColumn("messages", "is_outgoing", "INTEGER DEFAULT 0");
        ensureColumn("messages", "is_deleted", "INTEGER DEFAULT 0");
    }

    private void ensureColumn(String table, String column, String definition) throws SQLException {
        boolean exists = false;
        try (Statement statement = connection.createStatement();
             ResultSet rows = statement.executeQuery("PRAGMA table_info(" + table + ")")) {
            while (rows.next()) {
                if (column.equalsIgnoreCase(rows.getString("name"))) {
                    exists = true;
                    break;
                }
            }
        }
        if (!exists) {
            try (Statement statement = connection.createStatement()) {
                statement.executeUpdate("ALTER TABLE " + table + " ADD COLUMN " + column + " " + definition);
            }
        }
    }

    // -------------------------------------------------------- шифрование

    public String encrypt(String value) {
        if (value == null || value.isEmpty()) {
            return "";
        }
        if (value.startsWith(ENC_PREFIX)) {
            return value;
        }
        try {
            byte[] nonce = new byte[NONCE_BYTES];
            random.nextBytes(nonce);

            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, key, new GCMParameterSpec(GCM_TAG_BITS, nonce));
            byte[] encrypted = cipher.doFinal(value.getBytes(StandardCharsets.UTF_8));

            int payloadLength = encrypted.length - 16;
            byte[] payload = new byte[payloadLength];
            byte[] tag = new byte[16];
            System.arraycopy(encrypted, 0, payload, 0, payloadLength);
            System.arraycopy(encrypted, payloadLength, tag, 0, 16);

            Base64.Encoder encoder = Base64.getEncoder();
            return ENC_PREFIX + encoder.encodeToString(nonce) + ":"
                    + encoder.encodeToString(tag) + ":" + encoder.encodeToString(payload);
        } catch (Exception error) {
            throw new IllegalStateException("Ошибка шифрования", error);
        }
    }

    public String decrypt(String value) {
        if (value == null || value.isEmpty()) {
            return "";
        }
        if (!value.startsWith(ENC_PREFIX)) {
            return value;
        }
        try {
            String[] parts = value.substring(ENC_PREFIX.length()).split(":");
            if (parts.length != 3) {
                return "";
            }
            Base64.Decoder decoder = Base64.getDecoder();
            byte[] nonce = decoder.decode(parts[0]);
            byte[] tag = decoder.decode(parts[1]);
            byte[] payload = decoder.decode(parts[2]);

            byte[] combined = new byte[payload.length + tag.length];
            System.arraycopy(payload, 0, combined, 0, payload.length);
            System.arraycopy(tag, 0, combined, payload.length, tag.length);

            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, key, new GCMParameterSpec(GCM_TAG_BITS, nonce));
            return new String(cipher.doFinal(combined), StandardCharsets.UTF_8);
        } catch (Exception error) {
            return "";
        }
    }

    // ------------------------------------------------------- пользователи

    public Map<String, Object> getUser(long userId) {
        return queryOne("SELECT * FROM users WHERE user_id = ?", userId);
    }

    public boolean userExists(long userId) {
        return getUser(userId) != null;
    }

    /** Создаёт пользователя. Возвращает массив {user, isNew, referrerId}. */
    public Object[] createUser(long userId, String username, String firstName, Long referredBy) {
        Map<String, Object> existing = getUser(userId);
        if (existing != null) {
            return new Object[]{existing, false, null};
        }

        long now = now();
        Long referrerId = null;

        if (referredBy != null && referredBy != userId) {
            Map<String, Object> referrer = getUser(referredBy);
            if (referrer != null && number(referrer.get("is_banned")) != 1) {
                referrerId = referredBy;
            }
        }

        try (PreparedStatement statement = connection.prepareStatement("INSERT INTO users (user_id, username, first_name, subscription_until, trial_used, referred_by, created_at) VALUES (?, ?, ?, ?, 0, ?, ?)")) {
            statement.setLong(1, userId);
            statement.setString(2, username == null ? "" : username);
            statement.setString(3, firstName == null ? "" : firstName);
            statement.setLong(4, now + 3L * 86400);
            if (referrerId == null) {
                statement.setNull(5, java.sql.Types.INTEGER);
            } else {
                statement.setLong(5, referrerId);
            }
            statement.setLong(6, now);
            statement.executeUpdate();
        } catch (SQLException error) {
            throw new IllegalStateException("Не удалось создать пользователя", error);
        }

        if (referrerId != null) {
            execute("UPDATE users SET referrals_count = referrals_count + 1, "
                            + "subscription_until = MAX(subscription_until, ?) + ? WHERE user_id = ?",
                    now, 7L * 86400, referrerId);
        }

        return new Object[]{getUser(userId), true, referrerId};
    }

    public void addSubscriptionDays(long userId, int days, boolean premium) {
        Map<String, Object> user = getUser(userId);
        if (user == null) {
            return;
        }
        long now = now();
        long current = number(user.get("subscription_until"));
        long base = Math.max(current, now);
        long newUntil = base + (long) days * 86400;
        long premiumFlag = premium ? 1 : number(user.get("is_premium"));

        execute("UPDATE users SET subscription_until = ?, is_premium = ? WHERE user_id = ?",
                newUntil, premiumFlag, userId);
    }

    public void setPremium(long userId, boolean premium) {
        if (premium) {
            Map<String, Object> user = getUser(userId);
            long now = now();
            long base = Math.max(number(user == null ? 0 : user.get("subscription_until")), now);
            execute("UPDATE users SET is_premium = 1, subscription_until = ? WHERE user_id = ?",
                    base + 365L * 86400, userId);
        } else {
            execute("UPDATE users SET is_premium = 0 WHERE user_id = ?", userId);
        }
    }

    public void setBan(long userId, boolean banned) {
        execute("UPDATE users SET is_banned = ? WHERE user_id = ?", banned ? 1 : 0, userId);
    }

    public void resetSubscription(long userId) {
        execute("UPDATE users SET subscription_until = 0, is_premium = 0 WHERE user_id = ?", userId);
    }

    public void setFlag(long userId, String field, boolean value) {
        if (!List.of("notify_deleted", "notify_edited", "trial_used").contains(field)) {
            throw new IllegalArgumentException("Недопустимое поле: " + field);
        }
        execute("UPDATE users SET " + field + " = ? WHERE user_id = ?", value ? 1 : 0, userId);
    }

    public boolean hasPremium(long userId, List<Long> adminIds) {
        if (adminIds.contains(userId)) {
            return true;
        }
        Map<String, Object> user = getUser(userId);
        return user != null && number(user.get("is_premium")) == 1;
    }

    public Map<String, Long> getStats() {
        Map<String, Long> stats = new HashMap<>();
        long now = now();
        stats.put("totalUsers", scalar("SELECT COUNT(*) FROM users"));
        stats.put("activeSubs", scalar("SELECT COUNT(*) FROM users WHERE subscription_until > ? AND is_banned = 0", now));
        stats.put("premiumUsers", scalar("SELECT COUNT(*) FROM users WHERE is_premium = 1 AND is_banned = 0"));
        stats.put("bannedUsers", scalar("SELECT COUNT(*) FROM users WHERE is_banned = 1"));
        stats.put("activeConnections", scalar("SELECT COUNT(*) FROM business_connections WHERE is_enabled = 1"));
        return stats;
    }

    public List<Map<String, Object>> getAllUsers() {
        return queryList("SELECT * FROM users");
    }

    // -------------------------------------------------- бизнес-подключения

    public void saveBusinessConnection(long userId, String connectionId, boolean enabled) {
        execute("INSERT INTO business_connections (connection_id, user_id, user_chat_id, is_enabled, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(connection_id) DO UPDATE SET is_enabled = excluded.is_enabled, updated_at = excluded.updated_at",
                connectionId, userId, userId, enabled ? 1 : 0, now());
    }

    public Map<String, Object> getBusinessConnection(long userId) {
        return queryOne("SELECT * FROM business_connections WHERE user_id = ? AND is_enabled = 1 "
                + "ORDER BY updated_at DESC LIMIT 1", userId);
    }

    public Map<String, Object> getConnectionById(String connectionId) {
        return queryOne("SELECT * FROM business_connections WHERE connection_id = ? LIMIT 1", connectionId);
    }

    // ---------------------------------------------------------- сообщения

    public void saveMessage(String chatId, long messageId, long ownerId, Long senderId,
                            String senderName, String senderUsername, String peerName,
                            String peerUsername, String text, String mediaType, String fileId,
                            String fileName, boolean outgoing, long date) {
        execute("INSERT INTO messages (chat_id, message_id, owner_id, sender_id, sender_name, sender_username, peer_name, peer_username, text, media_type, file_id, file_name, is_outgoing, is_deleted, date) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?) ON CONFLICT(chat_id, message_id) DO UPDATE SET text = excluded.text, media_type = excluded.media_type, file_id = excluded.file_id, is_deleted = 0",
                chatId, messageId, ownerId, senderId, encrypt(senderName), encrypt(senderUsername),
                encrypt(peerName), encrypt(peerUsername), encrypt(text), mediaType, fileId,
                encrypt(fileName), outgoing ? 1 : 0, date);
    }

    public void markDeleted(String chatId, List<Long> messageIds) {
        if (messageIds == null || messageIds.isEmpty()) {
            return;
        }
        StringBuilder placeholders = new StringBuilder();
        List<Object> params = new ArrayList<>();
        params.add(chatId);
        for (Long id : messageIds) {
            if (placeholders.length() > 0) {
                placeholders.append(',');
            }
            placeholders.append('?');
            params.add(id);
        }
        execute("UPDATE messages SET is_deleted = 1 WHERE chat_id = ? AND message_id IN ("
                + placeholders + ")", params.toArray());
    }

    public void saveEditedText(String chatId, long messageId, String text) {
        execute("UPDATE messages SET text = ? WHERE chat_id = ? AND message_id = ?",
                encrypt(text), chatId, messageId);
    }

    public List<Map<String, Object>> getOwnerChats(long ownerId) {
        return queryList("SELECT chat_id, COUNT(*) AS msg_count, MAX(peer_name) AS peer_name, MAX(peer_username) AS peer_username FROM messages WHERE owner_id = ? GROUP BY chat_id ORDER BY MAX(date) DESC", ownerId);
    }

    public List<Map<String, Object>> getChatMessages(String chatId, long ownerId) {
        List<Map<String, Object>> rows = queryList(
                "SELECT * FROM messages WHERE chat_id = ? AND owner_id = ? ORDER BY date ASC",
                chatId, ownerId);

        for (Map<String, Object> row : rows) {
            for (String field : List.of("sender_name", "sender_username", "peer_name",
                    "peer_username", "text", "file_name", "local_path")) {
                Object value = row.get(field);
                if (value != null) {
                    row.put(field, decrypt(String.valueOf(value)));
                }
            }
        }
        return rows;
    }

    // ------------------------------------------------------------- утилиты

    private long now() {
        return System.currentTimeMillis() / 1000;
    }

    private static long number(Object value) {
        return value instanceof Number ? ((Number) value).longValue() : 0;
    }

    private void execute(String sql, Object... params) {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            bind(statement, params);
            statement.executeUpdate();
        } catch (SQLException error) {
            throw new IllegalStateException("SQL: " + error.getMessage(), error);
        }
    }

    private long scalar(String sql, Object... params) {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            bind(statement, params);
            try (ResultSet rows = statement.executeQuery()) {
                return rows.next() ? rows.getLong(1) : 0;
            }
        } catch (SQLException error) {
            throw new IllegalStateException("SQL: " + error.getMessage(), error);
        }
    }

    private Map<String, Object> queryOne(String sql, Object... params) {
        List<Map<String, Object>> rows = queryList(sql, params);
        return rows.isEmpty() ? null : rows.get(0);
    }

    private List<Map<String, Object>> queryList(String sql, Object... params) {
        List<Map<String, Object>> result = new ArrayList<>();
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            bind(statement, params);
            try (ResultSet rows = statement.executeQuery()) {
                int columns = rows.getMetaData().getColumnCount();
                while (rows.next()) {
                    Map<String, Object> row = new HashMap<>();
                    for (int i = 1; i <= columns; i++) {
                        row.put(rows.getMetaData().getColumnName(i), rows.getObject(i));
                    }
                    result.add(row);
                }
            }
        } catch (SQLException error) {
            throw new IllegalStateException("SQL: " + error.getMessage(), error);
        }
        return result;
    }

    private static void bind(PreparedStatement statement, Object... params) throws SQLException {
        for (int i = 0; i < params.length; i++) {
            Object param = params[i];
            if (param == null) {
                statement.setNull(i + 1, java.sql.Types.VARCHAR);
            } else if (param instanceof Long) {
                statement.setLong(i + 1, (Long) param);
            } else if (param instanceof Integer) {
                statement.setInt(i + 1, (Integer) param);
            } else {
                statement.setString(i + 1, String.valueOf(param));
            }
        }
    }

    @Override
    public void close() throws SQLException {
        connection.close();
    }
}
