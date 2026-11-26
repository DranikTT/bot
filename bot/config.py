"""Конфигурация бота: токен, админы, цены, лимиты и общие состояния.

Все значения читаются из переменных окружения (.env подхватывается
автоматически). Секретов в коде нет.
"""

from __future__ import annotations

import os
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
ASSETS_DIR = BASE_DIR / "assets"
MEDIA_DIR = BASE_DIR / "media_cache"
TEMP_DIR = BASE_DIR / "temp"

for directory in (MEDIA_DIR, TEMP_DIR):
    directory.mkdir(parents=True, exist_ok=True)


class ConfigError(RuntimeError):
    """Не хватает обязательных настроек."""


def _load_env_file() -> None:
    """Читает .env рядом с проектом, не перетирая уже заданные переменные."""
    env_path = BASE_DIR / ".env"
    if not env_path.exists():
        return

    for raw_line in env_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue

        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()

        if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
            value = value[1:-1]

        if key and key not in os.environ:
            os.environ[key] = value


_load_env_file()


def _env_number(name: str, fallback: float) -> float:
    try:
        value = float(os.environ.get(name, ""))
    except ValueError:
        return fallback
    return value if value > 0 else fallback


BOT_TOKEN = (os.environ.get("BOT_TOKEN") or "").strip()
if not BOT_TOKEN:
    raise ConfigError("Не задан BOT_TOKEN. Добавьте его в .env или в переменные окружения.")

ADMIN_IDS: list[int] = [
    int(part)
    for part in str(os.environ.get("ADMIN_IDS", "")).replace(",", " ").split()
    if part.isdigit() and int(part) > 0
]

# Цены в звёздах Telegram (XTR)
SUB_PRICES: dict[str, int] = {
    "days7": 25,
    "days30": 75,
    "days90": 150,
    "premium": 300,
}

MAX_EXPORT_MB = _env_number("MAX_EXPORT_MB", 48)
MAX_SINGLE_MEDIA_MB = _env_number("MAX_SINGLE_MEDIA_MB", 35)
MAX_IMPORT_MB = _env_number("MAX_IMPORT_MB", 50)
MAX_IMPORT_FILE_MB = _env_number("MAX_IMPORT_FILE_MB", 200)
MAX_IMPORTED_MEDIA_MB = _env_number("MAX_IMPORTED_MEDIA_MB", 100)

DATABASE_PATH = Path(os.environ.get("DATABASE_PATH") or BASE_DIR / "bot_database.sqlite")
DB_ENCRYPTION_KEY = (
    os.environ.get("DB_ENCRYPTION_KEY")
    or "SaveMod_AES256_Default_Secret_Key_2025_Secure!"
)

ALLOWED_UPDATES = [
    "message",
    "edited_message",
    "callback_query",
    "inline_query",
    "business_connection",
    "business_message",
    "edited_business_message",
    "deleted_business_messages",
    "pre_checkout_query",
]

# Бонусы
REGISTRATION_BONUS_DAYS = 3
REFERRAL_BONUS_DAYS = 7
PREMIUM_DURATION_DAYS = 365


def is_admin(user_id: int | None) -> bool:
    """Админ ли пользователь."""
    try:
        return int(user_id) in ADMIN_IDS
    except (TypeError, ValueError):
        return False


def banner_override(env_name: str, default: Path) -> Path:
    """Позволяет заменить картинку баннера через .env, не трогая код."""
    custom = (os.environ.get(env_name) or "").strip()
    return Path(custom).expanduser() if custom else default


# --------------------------------------------------------------------------
# Общие помощники состояний
# --------------------------------------------------------------------------

from typing import Any  # noqa: E402


class SelfDestructTracker:
    """Хранит последнее медиа по чату, чтобы команда «.» могла его вернуть."""

    def __init__(self, limit: int = 2000) -> None:
        self._items: dict[tuple[int, int], dict] = {}
        self._limit = limit

    def save_last_media(self, owner_id: int, chat_id: int, media_type: str,
                        file_id: str | None, file_name: str | None = None) -> None:
        self._items[(int(owner_id), int(chat_id))] = {
            "owner_id": int(owner_id),
            "chat_id": int(chat_id),
            "media_type": media_type,
            "file_id": file_id,
            "file_name": file_name,
        }
        while len(self._items) > self._limit:
            self._items.pop(next(iter(self._items)))

    def pop_last_media(self, owner_id: int, chat_id: int) -> dict | None:
        return self._items.pop((int(owner_id), int(chat_id)), None)

    def get_last_media(self, owner_id: int, chat_id: int) -> dict | None:
        return self._items.get((int(owner_id), int(chat_id)))


self_destruct_tracker = SelfDestructTracker()


def owner_of(message: Any) -> int | None:
    """Владелец бизнес-аккаунта, к которому пришло сообщение."""
    connection_id = getattr(message, "business_connection_id", None)
    if not connection_id:
        return None

    connection = db_connection(connection_id)
    if connection and connection.get("user_id"):
        return int(connection["user_id"])
    return None


def db_connection(connection_id: str) -> dict[str, Any] | None:
    """Ищет подключение по id (импорт внутри функции — избегаем цикла)."""
    from . import database as _db

    row = _db.get_connection().execute(
        "SELECT * FROM business_connections WHERE connection_id = ? LIMIT 1",
        (connection_id,),
    ).fetchone()
    return dict(row) if row else None
