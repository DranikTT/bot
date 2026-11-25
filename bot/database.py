"""Доступ к SQLite. Схема полностью совместима со старой версией на Node.js,
поэтому существующая bot_database.sqlite продолжит работать без миграции.
"""

from __future__ import annotations

import sqlite3
import threading
import time
from contextlib import contextmanager
from typing import Any, Iterable, Iterator

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from .config import DATABASE_PATH, DB_ENCRYPTION_KEY

ENC_PREFIX = "enc:v1:"

_local = threading.local()


def _connect() -> sqlite3.Connection:
    connection = sqlite3.connect(DATABASE_PATH, check_same_thread=False, timeout=5)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA journal_mode = WAL")
    connection.execute("PRAGMA synchronous = NORMAL")
    connection.execute("PRAGMA busy_timeout = 5000")
    connection.execute("PRAGMA foreign_keys = ON")
    return connection


def get_connection() -> sqlite3.Connection:
    """Соединение на текущий поток (sqlite3 не потокобезопасен)."""
    connection = getattr(_local, "connection", None)
    if connection is None:
        connection = _connect()
        _local.connection = connection
    return connection


@contextmanager
def transaction() -> Iterator[sqlite3.Connection]:
    connection = get_connection()
    with connection:
        yield connection


# --------------------------------------------------------------------------
# Шифрование (AES-256-GCM, формат совместим с прежней версией)
# --------------------------------------------------------------------------

import base64
import hashlib
import os as _os

_KEY = hashlib.sha256(DB_ENCRYPTION_KEY.encode()).digest()
_aes = AESGCM(_KEY)


def encrypt_str(value: Any) -> str:
    if value is None or value == "":
        return ""
    text = str(value)
    if text.startswith(ENC_PREFIX):
        return text

    nonce = _os.urandom(12)
    ciphertext = _aes.encrypt(nonce, text.encode(), None)
    tag, payload = ciphertext[-16:], ciphertext[:-16]
    return (
        f"{ENC_PREFIX}{base64.b64encode(nonce).decode()}:"
        f"{base64.b64encode(tag).decode()}:{base64.b64encode(payload).decode()}"
    )


def decrypt_str(value: Any) -> str:
    if value is None or value == "":
        return ""
    text = str(value)
    if not text.startswith(ENC_PREFIX):
        return text

    try:
        nonce_b64, tag_b64, data_b64 = text[len(ENC_PREFIX):].split(":")
        nonce = base64.b64decode(nonce_b64)
        tag = base64.b64decode(tag_b64)
        data = base64.b64decode(data_b64)
        return _aes.decrypt(nonce, data + tag, None).decode()
    except Exception:
        return ""


# --------------------------------------------------------------------------
# Схема
# --------------------------------------------------------------------------

SCHEMA = """
CREATE TABLE IF NOT EXISTS users (
    user_id INTEGER PRIMARY KEY,
    username TEXT DEFAULT '',
    first_name TEXT DEFAULT '',
    subscription_until INTEGER DEFAULT 0,
    is_premium INTEGER DEFAULT 0,
    trial_used INTEGER DEFAULT 0,
    notify_deleted INTEGER DEFAULT 1,
    notify_edited INTEGER DEFAULT 1,
    referred_by INTEGER DEFAULT NULL,
    referrals_count INTEGER DEFAULT 0,
    is_banned INTEGER DEFAULT 0,
    ban_reason TEXT DEFAULT '',
    mtproto_session TEXT DEFAULT '',
    mtproto_phone TEXT DEFAULT '',
    created_at INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS muted_chats (
    owner_id INTEGER,
    chat_id INTEGER,
    PRIMARY KEY (owner_id, chat_id)
);

CREATE TABLE IF NOT EXISTS bypass_chats (
    owner_id INTEGER,
    chat_id INTEGER,
    PRIMARY KEY (owner_id, chat_id)
);

CREATE TABLE IF NOT EXISTS business_connections (
    connection_id TEXT PRIMARY KEY,
    user_id INTEGER,
    user_chat_id INTEGER,
    can_reply INTEGER DEFAULT 0,
    is_enabled INTEGER DEFAULT 1,
    updated_at INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    chat_id INTEGER,
    message_id INTEGER,
    connection_id TEXT DEFAULT '',
    owner_id INTEGER,
    sender_id INTEGER,
    sender_name TEXT DEFAULT '',
    sender_username TEXT DEFAULT '',
    peer_name TEXT DEFAULT '',
    peer_username TEXT DEFAULT '',
    text TEXT DEFAULT '',
    edit_history TEXT DEFAULT '[]',
    reactions_history TEXT DEFAULT '[]',
    gift_json TEXT DEFAULT '',
    media_type TEXT DEFAULT NULL,
    file_id TEXT DEFAULT NULL,
    file_name TEXT DEFAULT NULL,
    local_path TEXT DEFAULT NULL,
    is_self_destruct INTEGER DEFAULT 0,
    is_deleted INTEGER DEFAULT 0,
    date INTEGER DEFAULT 0,
    is_outgoing INTEGER DEFAULT 0,
    PRIMARY KEY (chat_id, message_id)
);

CREATE INDEX IF NOT EXISTS idx_messages_owner_chat ON messages(owner_id, chat_id, date);
CREATE INDEX IF NOT EXISTS idx_messages_chat_date ON messages(chat_id, date);
CREATE INDEX IF NOT EXISTS idx_bconn_user ON business_connections(user_id, is_enabled);
"""

_COLUMNS = {
    "users": {
        "username": "TEXT DEFAULT ''",
        "first_name": "TEXT DEFAULT ''",
        "subscription_until": "INTEGER DEFAULT 0",
        "is_premium": "INTEGER DEFAULT 0",
        "trial_used": "INTEGER DEFAULT 0",
        "notify_deleted": "INTEGER DEFAULT 1",
        "notify_edited": "INTEGER DEFAULT 1",
        "referred_by": "INTEGER DEFAULT NULL",
        "referrals_count": "INTEGER DEFAULT 0",
        "is_banned": "INTEGER DEFAULT 0",
        "ban_reason": "TEXT DEFAULT ''",
        "mtproto_session": "TEXT DEFAULT ''",
        "mtproto_phone": "TEXT DEFAULT ''",
        "created_at": "INTEGER DEFAULT 0",
    },
    "business_connections": {
        "user_id": "INTEGER",
        "user_chat_id": "INTEGER",
        "can_reply": "INTEGER DEFAULT 0",
        "is_enabled": "INTEGER DEFAULT 1",
        "updated_at": "INTEGER DEFAULT 0",
    },
    "messages": {
        "connection_id": "TEXT DEFAULT ''",
        "owner_id": "INTEGER",
        "sender_id": "INTEGER",
        "sender_name": "TEXT DEFAULT ''",
        "sender_username": "TEXT DEFAULT ''",
        "peer_name": "TEXT DEFAULT ''",
        "peer_username": "TEXT DEFAULT ''",
        "text": "TEXT DEFAULT ''",
        "edit_history": "TEXT DEFAULT '[]'",
        "reactions_history": "TEXT DEFAULT '[]'",
        "gift_json": "TEXT DEFAULT ''",
        "media_type": "TEXT DEFAULT NULL",
        "file_id": "TEXT DEFAULT NULL",
        "file_name": "TEXT DEFAULT NULL",
        "local_path": "TEXT DEFAULT NULL",
        "is_self_destruct": "INTEGER DEFAULT 0",
        "is_deleted": "INTEGER DEFAULT 0",
        "date": "INTEGER DEFAULT 0",
        "is_outgoing": "INTEGER DEFAULT 0",
    },
}


def init_db() -> None:
    connection = get_connection()
    connection.executescript(SCHEMA)

    for table, columns in _COLUMNS.items():
        existing = {
            row["name"] for row in connection.execute(f"PRAGMA table_info({table})")
        }
        for name, definition in columns.items():
            if name not in existing:
                connection.execute(f"ALTER TABLE {table} ADD COLUMN {name} {definition}")

    connection.commit()


def _now() -> int:
    return int(time.time())


def _row(row: sqlite3.Row | None) -> dict[str, Any] | None:
    return dict(row) if row is not None else None


# --------------------------------------------------------------------------
# Пользователи
# --------------------------------------------------------------------------


def get_user(user_id: int | str) -> dict[str, Any] | None:
    try:
        uid = int(user_id)
    except (TypeError, ValueError):
        return None
    return _row(get_connection().execute("SELECT * FROM users WHERE user_id = ?", (uid,)).fetchone())


def create_user(user_id: int, username: str = "", first_name: str = "", referred_by: int | None = None) -> tuple[dict[str, Any], bool, int | None]:
    """Создаёт пользователя. Возвращает (user, is_new, referrer_id)."""
    uid = int(user_id)
    existing = get_user(uid)
    if existing:
        return existing, False, None

    now = _now()
    referrer_id: int | None = None

    if referred_by and int(referred_by) != uid:
        referrer = get_user(referred_by)
        if referrer and int(referrer.get("is_banned") or 0) != 1:
            referrer_id = int(referred_by)

    with transaction() as connection:
        connection.execute(
            """
            INSERT INTO users (user_id, username, first_name, subscription_until,
                               trial_used, referred_by, created_at)
            VALUES (?, ?, ?, ?, 0, ?, ?)
            """,
            (uid, username or "", first_name or "", now + 3 * 86400, referrer_id, now),
        )
        if referrer_id:
            connection.execute(
                """
                UPDATE users
                   SET referrals_count = referrals_count + 1,
                       subscription_until = MAX(subscription_until, ?) + ?
                 WHERE user_id = ?
                """,
                (now, 7 * 86400, referrer_id),
            )

    return get_user(uid) or {}, True, referrer_id


def add_subscription_days(user_id: int, days: int, is_premium: bool = False) -> bool:
    user = get_user(user_id)
    if not user:
        return False

    now = _now()
    current = int(user.get("subscription_until") or 0)
    base = current if current > now else now
    new_until = base + int(days) * 86400
    premium = 1 if is_premium else int(user.get("is_premium") or 0)

    with transaction() as connection:
        connection.execute(
            "UPDATE users SET subscription_until = ?, is_premium = ? WHERE user_id = ?",
            (new_until, premium, int(user_id)),
        )
    return True


def set_premium(user_id: int, is_premium: bool) -> bool:
    uid = int(user_id)
    if is_premium:
        user = get_user(uid)
        now = _now()
        current = int((user or {}).get("subscription_until") or 0)
        base = current if current > now else now
        with transaction() as connection:
            connection.execute(
                "UPDATE users SET is_premium = 1, subscription_until = ? WHERE user_id = ?",
                (base + 365 * 86400, uid),
            )
    else:
        with transaction() as connection:
            connection.execute(
                "UPDATE users SET is_premium = 0 WHERE user_id = ?", (uid,)
            )
    return True


def set_ban(user_id: int, is_banned: bool) -> bool:
    with transaction() as connection:
        cursor = connection.execute(
            "UPDATE users SET is_banned = ? WHERE user_id = ?",
            (1 if is_banned else 0, int(user_id)),
        )
        return cursor.rowcount > 0


def reset_subscription(user_id: int) -> bool:
    with transaction() as connection:
        cursor = connection.execute(
            "UPDATE users SET subscription_until = 0, is_premium = 0 WHERE user_id = ?",
            (int(user_id),),
        )
        return cursor.rowcount > 0


def set_flag(user_id: int, field: str, value: bool | int) -> bool:
    if field not in {"notify_deleted", "notify_edited", "trial_used"}:
        raise ValueError(f"Недопустимое поле: {field}")

    with transaction() as connection:
        cursor = connection.execute(
            f"UPDATE users SET {field} = ? WHERE user_id = ?",
            (1 if value else 0, int(user_id)),
        )
        return cursor.rowcount > 0


def has_premium(user_id: int, admin_ids: Iterable[int] = ()) -> bool:
    uid = int(user_id)
    if uid in {int(admin) for admin in admin_ids}:
        return True
    user = get_user(uid)
    return bool(user and int(user.get("is_premium") or 0) == 1)


def get_stats() -> dict[str, int]:
    connection = get_connection()
    now = _now()

    def scalar(query: str, params: tuple = ()) -> int:
        row = connection.execute(query, params).fetchone()
        return int(row[0]) if row else 0

    return {
        "total_users": scalar("SELECT COUNT() FROM users"),
        "active_subs": scalar(
            "SELECT COUNT() FROM users WHERE subscription_until > ? AND is_banned = 0", (now,)
        ),
        "premium_users": scalar(
            "SELECT COUNT() FROM users WHERE is_premium = 1 AND is_banned = 0"
        ),
        "banned_users": scalar("SELECT COUNT() FROM users WHERE is_banned = 1"),
        "active_connections": scalar(
            "SELECT COUNT(*) FROM business_connections WHERE is_enabled = 1"
        ),
    }


def get_all_users() -> list[dict[str, Any]]:
    return [dict(row) for row in get_connection().execute("SELECT * FROM users").fetchall()]


# --------------------------------------------------------------------------
# Бизнес-подключения
# --------------------------------------------------------------------------


def save_business_connection(user_id: int, connection_id: str, is_enabled: bool) -> None:
    uid = int(user_id)
    with transaction() as connection:
        connection.execute(
            """
            INSERT INTO business_connections
                (connection_id, user_id, user_chat_id, is_enabled, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(connection_id) DO UPDATE SET
                is_enabled = excluded.is_enabled,
                updated_at = excluded.updated_at
            """,
            (connection_id, uid, uid, 1 if is_enabled else 0, _now()),
        )


def get_business_connection(user_id: int) -> dict[str, Any] | None:
    return _row(
        get_connection()
        .execute(
            "SELECT * FROM business_connections "
            "WHERE user_id = ? AND is_enabled = 1 ORDER BY updated_at DESC LIMIT 1",
            (int(user_id),),
        )
        .fetchone()
    )


# --------------------------------------------------------------------------
# Сообщения
# --------------------------------------------------------------------------


def get_owner_chats(user_id: int) -> list[dict[str, Any]]:
    rows = get_connection().execute(
        """
        SELECT chat_id,
               COUNT(*) AS msg_count,
               MAX(peer_name) AS peer_name,
               MAX(peer_username) AS peer_username
          FROM messages
         WHERE owner_id = ?
         GROUP BY chat_id
         ORDER BY MAX(date) DESC
        """,
        (int(user_id),),
    ).fetchall()

    chats: list[dict[str, Any]] = []
    for row in rows:
        item = dict(row)
        item["peer_name"] = decrypt_str(item.get("peer_name")) or ""
        item["peer_username"] = decrypt_str(item.get("peer_username")) or ""
        chats.append(item)
    return chats


def _decrypt_message_row(row: sqlite3.Row | None) -> dict[str, Any] | None:
    if row is None:
        return None

    data = dict(row)
    for field in (
        "sender_name",
        "sender_username",
        "peer_name",
        "peer_username",
        "text",
        "edit_history",
        "reactions_history",
        "gift_json",
        "file_name",
        "local_path",
    ):
        if data.get(field) is not None:
            data[field] = decrypt_str(data[field])
    return data


def get_chat_messages(chat_id: int | str, owner_id: int) -> list[dict[str, Any]]:
    rows = get_connection().execute(
        "SELECT * FROM messages WHERE chat_id = ? AND owner_id = ? ORDER BY date ASC",
        (chat_id, int(owner_id)),
    ).fetchall()

    return [message for message in (_decrypt_message_row(row) for row in rows) if message]


def save_message(
    chat_id: int | str,
    message_id: int,
    owner_id: int,
    sender_id: int | None,
    sender_name: str = "",
    sender_username: str = "",
    peer_name: str = "",
    peer_username: str = "",
    text: str = "",
    media_type: str | None = None,
    file_id: str | None = None,
    file_name: str | None = None,
    local_path: str | None = None,
    is_self_destruct: bool = False,
    date: int | None = None,
    is_outgoing: bool = False,
) -> None:
    """Сохраняет сообщение. Текстовые поля шифруются, как и раньше."""
    with transaction() as connection:
        connection.execute(
            """
            INSERT INTO messages (
                chat_id, message_id, owner_id, sender_id, sender_name, sender_username,
                peer_name, peer_username, text, media_type, file_id, file_name,
                local_path, is_self_destruct, is_deleted, date, is_outgoing
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
            ON CONFLICT(chat_id, message_id) DO UPDATE SET
                text = excluded.text,
                media_type = excluded.media_type,
                file_id = excluded.file_id,
                file_name = excluded.file_name,
                local_path = excluded.local_path,
                is_deleted = 0
            """,
            (
                str(chat_id),
                int(message_id),
                int(owner_id),
                int(sender_id) if sender_id else None,
                encrypt_str(sender_name),
                encrypt_str(sender_username),
                encrypt_str(peer_name),
                encrypt_str(peer_username),
                encrypt_str(text),
                media_type,
                file_id,
                encrypt_str(file_name) if file_name else None,
                encrypt_str(local_path) if local_path else None,
                1 if is_self_destruct else 0,
                int(date or _now()),
                1 if is_outgoing else 0,
            ),
        )


def mark_messages_deleted(chat_id: int | str, message_ids: Iterable[int]) -> int:
    ids = [int(mid) for mid in message_ids]
    if not ids:
        return 0

    placeholders = ",".join("?" for _ in ids)
    with transaction() as connection:
        cursor = connection.execute(
            f"UPDATE messages SET is_deleted = 1 "
            f"WHERE chat_id = ? AND message_id IN ({placeholders})",
            (str(chat_id), *ids),
        )
        return cursor.rowcount


def save_edited_message(chat_id: int | str, message_id: int, text: str) -> None:
    with transaction() as connection:
        connection.execute(
            "UPDATE messages SET text = ? WHERE chat_id = ? AND message_id = ?",
            (encrypt_str(text), str(chat_id), int(message_id)),
        )


def save_business_message(chat_id: int | str, message_id: int, owner_id: int, text: str = "", sender_id: int | None = None, sender_name: str = "", media_type: str | None = None, file_id: str | None = None, date: int | None = None) -> None:
    save_message(
        chat_id=chat_id,
        message_id=message_id,
        owner_id=owner_id,
        sender_id=sender_id,
        sender_name=sender_name,
        text=text,
        media_type=media_type,
        file_id=file_id,
        date=date,
    )
