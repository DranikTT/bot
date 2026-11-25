"""Telegram Business: подключения, входящие/изменённые/удалённые сообщения.

Здесь же — перехват «сгорающих» медиа: если пользователь отправляет в чат
бизнес-аккаунта точку (или запятую), бот сохраняет последнее медиа.
"""

from __future__ import annotations

import logging

from aiogram import Bot, Router
from aiogram.types import BusinessConnection, BusinessMessagesDeleted, Message, MessageReactionUpdated

from . import database as db
from .config import ADMIN_IDS, is_admin, owner_of, self_destruct_tracker
from .menu import escape_html  # noqa: F401 — реэкспорт для хендлеров

log = logging.getLogger(__name__)
router = Router(name="business")

MEDIA_FIELDS = (
    "photo",
    "video",
    "video_note",
    "animation",
    "voice",
    "audio",
    "sticker",
    "document",
)

DOT_MESSAGES = {".", ","}


def _media_type(message: Message) -> tuple[str | None, str | None, str | None]:
    """Возвращает (media_type, file_id, file_name)."""
    if message.photo:
        return "photo", message.photo[-1].file_id, None
    if message.video:
        return "video", message.video.file_id, message.video.file_name
    if message.video_note:
        return "video_note", message.video_note.file_id, None
    if message.animation:
        return "animation", message.animation.file_id, message.animation.file_name
    if message.voice:
        return "voice", message.voice.file_id, None
    if message.audio:
        return "audio", message.audio.file_id, message.audio.file_name
    if message.sticker:
        return "sticker", message.sticker.file_id, None
    if message.document:
        return "document", message.document.file_id, message.document.file_name
    return None, None, None


def _peer_title(message: Message) -> str:
    chat = message.chat
    if chat.title:
        return chat.title
    full = " ".join(part for part in (chat.first_name, chat.last_name) if part)
    return full or (f"@{chat.username}" if chat.username else f"Чат {chat.id}")


def _sender_name(message: Message) -> str:
    user = message.from_user
    if not user:
        return ""
    return " ".join(part for part in (user.first_name, user.last_name) if part) or user.username or ""


@router.business_connection()
async def on_business_connection(connection: BusinessConnection) -> None:
    try:
        db.save_business_connection(
            user_id=connection.user.id,
            connection_id=connection.id,
            is_enabled=bool(connection.is_enabled),
        )
        log.info(
            "[Business] %s → %s (%s)",
            connection.user.id,
            connection.id,
            "включено" if connection.is_enabled else "выключено",
        )
    except Exception as error:  # noqa: BLE001 — событие не должно ронять поллер
        log.exception("[Business] Ошибка обновления подключения: %s", error)


@router.business_message()
async def on_business_message(message: Message) -> None:
    try:
        if message.business_connection_id is None:
            return

        owner_id = owner_of(message)
        if owner_id is None:
            return

        media_type, file_id, file_name = _media_type(message)
        text = message.text or message.caption or ""

        db.save_message(
            chat_id=message.chat.id,
            message_id=message.message_id,
            owner_id=owner_id,
            sender_id=message.from_user.id if message.from_user else None,
            sender_name=_sender_name(message),
            sender_username=message.from_user.username if message.from_user else "",
            peer_name=_peer_title(message),
            peer_username=message.chat.username or "",
            text=text,
            media_type=media_type,
            file_id=file_id,
            file_name=file_name,
            is_self_destruct=bool(message.has_media_spoiler),
            date=int(message.date.timestamp()),
        )

        if media_type:
            self_destruct_tracker.save_last_media(
                owner_id=owner_id,
                chat_id=message.chat.id,
                media_type=media_type,
                file_id=file_id,
                file_name=file_name,
            )
    except Exception as error:  # noqa: BLE001
        log.exception("[Business] Ошибка сохранения сообщения: %s", error)


@router.edited_business_message()
async def on_edited_business_message(message: Message) -> None:
    try:
        if message.business_connection_id is None:
            return
        db.save_edited_message(
            chat_id=message.chat.id,
            message_id=message.message_id,
            text=message.text or message.caption or "",
        )
    except Exception as error:  # noqa: BLE001
        log.exception("[Business] Ошибка сохранения изменения: %s", error)


@router.deleted_business_messages()
async def on_deleted_business_messages(event: BusinessMessagesDeleted) -> None:
    try:
        db.mark_messages_deleted(event.chat.id, event.message_ids)
    except Exception as error:  # noqa: BLE001
        log.exception("[Business] Ошибка отметки удаления: %s", error)


@router.message_reaction()
async def on_reaction(event: MessageReactionUpdated) -> None:
    """Заглушка: реакции пока только логируем, чтобы не терять события."""
    log.debug("[Business] Реакция в чате %s", event.chat.id)


async def send_saved_media(bot: Bot, owner_id: int, chat_id: int) -> bool:
    """Досылает владельцу последнее «сгоревшее» медиа (команда «.»)."""
    entry = self_destruct_tracker.pop_last_media(owner_id, chat_id)
    if not entry:
        return False

    try:
        await bot.copy_message(
            chat_id=owner_id,
            from_chat_id=entry.chat_id,
            message_id=entry.message_id,
        )
        return True
    except Exception as error:  # noqa: BLE001
        log.warning("[Business] Не удалось переслать медиа: %s", error)
        return False


def is_admin_or_owner(user_id: int | None) -> bool:
    return is_admin(user_id) or (user_id in ADMIN_IDS)
