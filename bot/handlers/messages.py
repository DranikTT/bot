"""Личные сообщения: перехват «сгорающих» медиа по точке/запятой."""

from __future__ import annotations

import logging

from aiogram import Bot, F, Router
from aiogram.types import Message

from ..business import send_saved_media
from ..config import self_destruct_tracker

log = logging.getLogger(__name__)
router = Router(name="messages")

SELF_DESTRUCT_TRIGGERS = {".", ","}


@router.message(F.chat.type == "private", F.text.in_({".", ","}))
async def on_dot(message: Message, bot: Bot) -> None:
    """Точка в чате с ботом — дослать последнее сохранённое медиа."""
    user_id = message.from_user.id

    entry = self_destruct_tracker.get_last_media(user_id, message.chat.id)
    if not entry:
        await message.answer(
            "Пока нечего отправлять: медиа появится здесь, когда бот "
            "зафиксирует сгорающее сообщение в вашем бизнес-чате."
        )
        return

    if await send_saved_media(bot, user_id, message.chat.id):
        await message.answer("📤 Отправил сохранённое медиа.")
    else:
        await message.answer("Не удалось переслать медиа — оно могло уже исчезнуть.")


@router.message(F.chat.type == "private", F.text.startswith("/"))
async def on_unknown_command(message: Message) -> None:
    await message.answer(
        "Неизвестная команда. Доступно: /menu, /export, /archive, /connect, /admin."
    )
