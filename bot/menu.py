"""Тексты меню и отправка с баннерами.

Про DOCUMENT_INVALID
--------------------
Прошлая версия всегда вызывала sendPhoto(), подставляя путь к баннеру.
Если файла на диске не было (папка image/ в репозитории отсутствует),
Telegram отвечал «400: DOCUMENT_INVALID» и меню вообще не отправлялось.
Здесь баннер отправляется только если файл реально существует и
открывается как изображение; в любом другом случае меню уходит обычным
текстом. После первой успешной отправки кэшируется file_id — дальше
фото не перезагружается.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from aiogram import Bot
from aiogram.enums import ChatAction
from aiogram.exceptions import TelegramAPIError
from aiogram.types import BufferedInputFile, FSInputFile, InlineKeyboardMarkup, Message

from . import database as db
from .config import ADMIN_IDS, ASSETS_DIR, PREMIUM_DURATION_DAYS, banner_override, is_admin
from .keyboards import (
    admin_keyboard,
    archive_types_keyboard,
    connect_keyboard,
    create_bot_keyboard,
    invite_keyboard,
    main_menu_keyboard,
    premium_keyboard,
    settings_keyboard,
    tariffs_keyboard,
)

log = logging.getLogger(__name__)

# Имена файлов баннеров. Можно переопределить в .env: BANNER_MAIN=/path/file.jpg
BANNERS: dict[str, Path] = {
    "main": banner_override("BANNER_MAIN", ASSETS_DIR / "banner_main.jpg"),
    "settings": banner_override("BANNER_SETTINGS", ASSETS_DIR / "banner_settings.jpg"),
    "subscription": banner_override("BANNER_SUBSCRIPTION", ASSETS_DIR / "banner_subscription.jpg"),
    "admin": banner_override("BANNER_ADMIN", ASSETS_DIR / "banner_admin.jpg"),
}

# Кэш file_id: после первой отправки не гоняем картинку повторно.
BANNER_FILE_IDS: dict[str, str] = {}

ARCHIVE_TYPE_LABELS: dict[str, str] = {
    "text": "💬 Текст",
    "photo": "🖼 Фото",
    "video": "🎬 Видео",
    "audio": "🎵 Аудио",
    "document": "📎 Файлы",
    "animation": "🎞 Гиф",
    "video_note": "🔘 Кружки",
    "voice": "🎤 Голос",
    "location": "📍 Гео",
    "sticker": "✨ Стикеры",
}


@dataclass(slots=True)
class Menu:
    """Готовое меню: текст + клавиатура + необязательный баннер."""

    text: str
    keyboard: InlineKeyboardMarkup | None = None
    banner: str | None = None
    extra: dict[str, Any] = field(default_factory=dict)


# --------------------------------------------------------------------------
# Форматирование
# --------------------------------------------------------------------------


def escape_html(value: Any) -> str:
    if value is None:
        return ""
    return (
        str(value)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def format_user_nick(user: dict[str, Any] | None) -> str:
    if not user:
        return "Пользователь"

    name = " ".join(
        part for part in (user.get("first_name"),) if part
    ) or "Пользователь"
    username = user.get("username")
    safe_name = escape_html(name)
    return f"{safe_name} (@{escape_html(username)})" if username else safe_name


def _format_dt(timestamp: int | float | None) -> str:
    if not timestamp:
        return "нет"
    from datetime import datetime

    return datetime.fromtimestamp(int(timestamp)).strftime("%d.%m.%Y %H:%M")


def format_sub_status(user: dict[str, Any] | None) -> str:
    if not user:
        return "не активна"
    if int(user.get("is_banned") or 0) == 1:
        return "заблокирована (бан)"
    if is_admin(user.get("user_id")):
        return "безлимит (админ)"

    from datetime import datetime

    until = int(user.get("subscription_until") or 0)
    now = int(datetime.now().timestamp())

    if until > now:
        days_left = -(-(until - now) // 86400)
        premium = "Премиум " if int(user.get("is_premium") or 0) == 1 else ""
        return f"{premium}активна до {_format_dt(until)} ({days_left} дн.)"
    return "не активна"


# --------------------------------------------------------------------------
# Тексты меню
# --------------------------------------------------------------------------


def main_menu(user: dict[str, Any] | None = None, *, user_id: int | None = None) -> Menu:
    """Главное меню. Умеет работать даже без записи в БД."""
    if user_id is None:
        user_id = int(user["user_id"]) if user else 0
    user = user or {"user_id": user_id}
    connection = db.get_business_connection(user_id)
    connected = bool(connection and int(connection.get("is_enabled") or 0) == 1)

    text = (
        f"👋 Привет, <b>{format_user_nick(user)}</b>!\n\n"
        f"🛡 Статус подписки: <b>{format_sub_status(user)}</b>\n"
        f"📎 Подключение: <b>{'подключён' if connected else 'не подключён'}</b>\n\n"
        f"🔽 Выберите раздел:"
    )
    return Menu(text, main_menu_keyboard(user_id), "main")


def settings_menu(user_id: int) -> Menu:
    user = db.get_user(user_id) or {}
    deleted_on = int(user.get("notify_deleted", 1) or 0) == 1
    edited_on = int(user.get("notify_edited", 1) or 0) == 1

    text = (
        "⚙️ <b>Настройки</b>\n\n"
        f"• Уведомления об удалённых: <b>{'включены' if deleted_on else 'выключены'}</b>\n"
        f"• Уведомления об изменённых: <b>{'включены' if edited_on else 'выключены'}</b>\n"
        "• Перехват сгорающих медиа (точка <code>.</code> или <code>,</code>): <b>активен</b>"
    )
    return Menu(text, settings_keyboard(user_id), "settings")


def connect_menu(bot_username: str) -> Menu:
    text = (
        "🔌 <b>Подключение к Telegram Business</b>\n\n"
        "Подключение выполняет владелец аккаунта вручную — бот не может "
        "сделать это сам.\n\n"
        "1. Откройте <b>Профиль</b> → <b>Изменить</b>\n"
        "2. Перейдите в <b>Автоматизация чатов</b>\n"
        f"3. Укажите юзернейм <code>@{escape_html(bot_username)}</code>\n"
        "4. Нажмите <b>Добавить</b> и выдайте права на чтение и ответы\n\n"
        "После этого нажмите «Проверить подключение»."
    )
    return Menu(text, connect_keyboard())


def tariffs_menu(user_id: int) -> Menu:
    user = db.get_user(user_id)
    text = (
        "💎 <b>Тарифы</b>\n\n"
        f"Ваш статус: <b>{format_sub_status(user)}</b>\n\n"
        "Оплата проходит звёздами Telegram. Подписка продлевается, "
        "а не заменяет текущую."
    )
    return Menu(text, tariffs_keyboard(), "subscription")


def premium_menu(user_id: int) -> Menu:
    is_premium = db.has_premium(user_id, ADMIN_IDS)
    text = (
        "⭐️ <b>Премиум — максимальный контроль</b>\n\n"
        f"Статус: <b>{'активен ✅' if is_premium else 'не подключён ❌'}</b>\n\n"
        f"Премиум оформляется на {PREMIUM_DURATION_DAYS} дней.\n\n"
        "Команды <code>.nah</code>, <code>.love</code>, <code>.bypass</code>, "
        "<code>.mute</code> и <code>.unmute</code> работают и с обычной подпиской."
    )
    return Menu(text, premium_keyboard(), "subscription")


def invite_menu(user: dict[str, Any], bot_username: str) -> Menu:
    user_id = int(user["user_id"])
    link = f"https://t.me/{bot_username}?start=ref_{user_id}"

    text = (
        "👥 <b>Пригласить друга</b>\n\n"
        "• Вам за каждого друга: <b>+7 дней подписки</b>\n"
        "• Другу при регистрации: <b>+3 дня подписки</b>\n\n"
        f"Приглашено: <b>{int(user.get('referrals_count') or 0)}</b>\n\n"
        f"Ваша ссылка:\n<code>{escape_html(link)}</code>"
    )
    return Menu(text, invite_keyboard(bot_username))


def create_bot_menu(bot_username: str) -> Menu:
    text = (
        "🤖 <b>Создать бота</b>\n\n"
        "1. Откройте <b>@BotFather</b> и отправьте <code>/newbot</code>\n"
        "2. Придумайте имя и юзернейм\n"
        "3. Скопируйте токен и вставьте его в <code>.env</code>:\n"
        "   <code>BOT_TOKEN=123456:AA...</code>\n"
        "4. Перезапустите бота\n\n"
        f"Этот бот уже работает как <code>@{escape_html(bot_username)}</code> — "
        "если нужно, подключите его в Telegram Business."
    )
    return Menu(text, create_bot_keyboard(bot_username))


def archive_types_menu(chat_id: str, chat_title: str, selected: set[str]) -> Menu:
    text = (
        "📁 <b>Настройка экспорта</b>\n"
        f"👤 Собеседник: <b>{escape_html(chat_title)}</b> (<code>{escape_html(chat_id)}</code>)\n\n"
        "<b>Выберите типы сообщений:</b>"
    )
    return Menu(text, archive_types_keyboard(selected, ARCHIVE_TYPE_LABELS))


def admin_menu() -> Menu:
    stats = db.get_stats()
    text = (
        "👑 <b>Админ-панель</b>\n\n"
        f"📊 Пользователей: <b>{stats['total_users']}</b>\n"
        f"💎 Активных подписок: <b>{stats['active_subs']}</b>\n"
        f"⭐️ Премиум: <b>{stats['premium_users']}</b>\n"
        f"🚫 Забанено: <b>{stats['banned_users']}</b>\n"
        f"🔌 Подключений: <b>{stats['active_connections']}</b>\n\n"
        "Выберите действие:"
    )
    return Menu(text, admin_keyboard(), "admin")


# --------------------------------------------------------------------------
# Проверка баннера (лечит DOCUMENT_INVALID)
# --------------------------------------------------------------------------


# path -> (mtime, valid): кэш привязан к файлу, а не к ключу,
# поэтому замена картинки или пути подхватывается автоматически.
_BANNER_CACHE: dict[str, tuple[float, bool]] = {}

# Telegram принимает в sendPhoto JPEG/PNG; прочие форматы рискуют DOCUMENT_INVALID
_ALLOWED_SUFFIXES = {".jpg", ".jpeg", ".png"}


def banner_is_valid(key: str) -> bool:
    """Проверяет, что баннер существует и реально является картинкой.

    Именно отсутствие файла давало «400: DOCUMENT_INVALID» на sendPhoto.
    Результат кэшируется, чтобы не читать файл на каждое меню.
    """
    path = BANNERS.get(key)
    if not path:
        return False

    try:
        stat = path.stat()
    except OSError:
        return False

    cache_key = str(path.resolve())
    cached = _BANNER_CACHE.get(cache_key)
    if cached and cached[0] == stat.st_mtime:
        return cached[1]

    valid = stat.st_size > 0 and _looks_like_image(path)
    _BANNER_CACHE[cache_key] = (stat.st_mtime, valid)
    return valid


def _looks_like_image(path: Path) -> bool:
    """Проверяет сигнатуру (magic bytes) — надёжнее, чем расширение."""
    try:
        header = path.read_bytes()[:16]
    except OSError:
        return False

    is_jpeg = header.startswith(b"\xff\xd8\xff")
    is_png = header.startswith(b"\x89PNG\r\n\x1a\n")

    if not (is_jpeg or is_png):
        return False

    # Если Pillow доступен — дополнительно убеждаемся, что файл открывается
    try:
        from PIL import Image

        with Image.open(path) as image:
            image.verify()
    except ImportError:
        pass
    except Exception:
        return False

    return True


def reset_banner_cache() -> None:
    """Сбрасывает кэш проверки (например, после замены картинки)."""
    _BANNER_CACHE.clear()
    BANNER_FILE_IDS.clear()


def _photo_source(key: str) -> FSInputFile | BufferedInputFile | str:
    file_id = BANNER_FILE_IDS.get(key)
    if file_id:
        return file_id
    return FSInputFile(BANNERS[key])


# --------------------------------------------------------------------------
# Отправка
# --------------------------------------------------------------------------


async def _safe_photo_edit(
    bot: Bot,
    chat_id: int,
    message_id: int,
    menu: Menu,
    key: str,
) -> bool:
    try:
        from aiogram.types import InputMediaPhoto

        media = InputMediaPhoto(
            media=_photo_source(key),
            caption=menu.text,
            parse_mode="HTML",
        )
        await bot.edit_message_media(
            chat_id=chat_id,
            message_id=message_id,
            media=media,
            reply_markup=menu.keyboard,
        )
        return True
    except TelegramAPIError as error:
        if "message is not modified" in str(error).lower():
            return True
        log.debug("edit_message_media не сработал (%s), отправляю заново", error)
        return False


async def show_menu(
    bot: Bot,
    chat_id: int,
    menu: Menu,
    *,
    message_id: int | None = None,
) -> Message | None:
    """Показывает меню, по возможности переиспользуя сообщение.

    Работает в трёх режимах:
      1. есть message_id и баннер — заменяем медиа (не плодим сообщения);
      2. есть message_id без баннера — просто edit_text;
      3. нет message_id — отправляем новое (фото, если баннер валиден).
    """
    key = menu.banner
    send_keyboard = menu.keyboard

    if message_id:
        if key and banner_is_valid(key):
            if await _safe_photo_edit(bot, chat_id, message_id, menu, key):
                return None

        try:
            return await bot.edit_message_text(
                chat_id=chat_id,
                message_id=message_id,
                text=menu.text,
                reply_markup=send_keyboard,
                parse_mode="HTML",
                disable_web_page_preview=True,
            )
        except TelegramAPIError as error:
            if "message is not modified" in str(error).lower():
                return None
            log.debug("edit_message_text не сработал (%s), отправляю заново", error)

    # Баннер отправляем только после проверки файла — иначе DOCUMENT_INVALID
    if key and banner_is_valid(key):
        try:
            message = await bot.send_photo(
                chat_id=chat_id,
                photo=_photo_source(key),
                caption=menu.text,
                reply_markup=send_keyboard,
                parse_mode="HTML",
            )
            if message.photo:
                BANNER_FILE_IDS[key] = message.photo[-1].file_id
            return message
        except TelegramAPIError as error:
            # Даже проверенный файл может не понравиться Telegram
            # (например, нестандартный JPEG). Показываем меню без картинки,
            # чтобы кнопки в любом случае остались рабочими.
            log.warning("[sendMenu] Баннер «%s» не отправлен: %s", key, error)

    return await bot.send_message(
        chat_id=chat_id,
        text=menu.text,
        reply_markup=send_keyboard,
        parse_mode="HTML",
        disable_web_page_preview=True,
    )


async def replace_menu(bot: Bot, chat_id: int, message_id: int | None, menu: Menu) -> Message | None:
    """Удаляет старое сообщение и отправляет новое (для смены типа сообщения)."""
    if message_id:
        try:
            await bot.delete_message(chat_id=chat_id, message_id=message_id)
        except TelegramAPIError:
            pass
    return await show_menu(bot, chat_id, menu)


async def typing(bot: Bot, chat_id: int) -> None:
    try:
        await bot.send_chat_action(chat_id, ChatAction.TYPING)
    except TelegramAPIError:
        pass


def photo_bytes(path: Path) -> BufferedInputFile | None:
    """Читает картинку в память — нужно там, где файл удаляется сразу."""
    try:
        return BufferedInputFile(path.read_bytes(), filename=path.name)
    except OSError:
        return None


__all__ = [
    "ARCHIVE_TYPE_LABELS",
    "BANNERS",
    "BANNER_FILE_IDS",
    "Menu",
    "admin_menu",
    "archive_types_menu",
    "banner_is_valid",
    "connect_menu",
    "create_bot_menu",
    "escape_html",
    "format_sub_status",
    "format_user_nick",
    "invite_menu",
    "main_menu",
    "premium_menu",
    "replace_menu",
    "settings_menu",
    "show_menu",
    "tariffs_menu",
    "typing",
]
