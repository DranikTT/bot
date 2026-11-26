"""Оплата звёздами: pre_checkout и успешный платёж."""

from __future__ import annotations

import logging

from aiogram import Bot, F, Router
from aiogram.types import (
    InlineKeyboardButton,
    InlineKeyboardMarkup,
    InlineQuery,
    InlineQueryResultArticle,
    InputTextMessageContent,
    Message,
    PreCheckoutQuery,
)

from .. import database as db
from ..config import PREMIUM_DURATION_DAYS
from ..menu import main_menu, show_menu

log = logging.getLogger(__name__)
router = Router(name="payments")

PAYLOAD_DAYS = {
    "sub_7": 7,
    "sub_30": 30,
    "sub_90": 90,
    "sub_premium": PREMIUM_DURATION_DAYS,
}


@router.pre_checkout_query()
async def on_pre_checkout(query: PreCheckoutQuery) -> None:
    await query.answer(ok=True)


@router.message(F.successful_payment)
async def on_successful_payment(message: Message, bot: Bot) -> None:
    payment = message.successful_payment
    if not payment:
        return

    payload = (payment.invoice_payload or "").split("_")
    key = "_".join(payload[:2])
    days = PAYLOAD_DAYS.get(key, 30)
    is_premium = key == "sub_premium"

    user_id = message.from_user.id
    db.create_user(user_id, username=message.from_user.username or "", first_name=message.from_user.first_name or "")

    try:
        db.add_subscription_days(user_id, days, is_premium=is_premium)
    except Exception as error:  # noqa: BLE001
        log.exception("[Payment] Не удалось начислить подписку: %s", error)
        await message.answer(
            "❌ Оплата получена, но подписку не удалось обновить. "
            "Напишите администратору."
        )
        return

    user = db.get_user(user_id) or {}
    await message.answer(
        f"✅ Оплата получена! Начислено <b>{days} дней</b>"
        f"{' и Премиум ⭐️' if is_premium else ''}.",
        parse_mode="HTML",
    )
    await show_menu(bot, message.chat.id, main_menu(user))


@router.inline_query()
async def on_inline_query(query: InlineQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    link = f"https://t.me/{username}?start=ref_{query.from_user.id}"

    result = InlineQueryResultArticle(
        id=f"ref_{query.from_user.id}",
        title="Пригласить друга и получить +7 дней",
        description="Друг получит +3 дня при регистрации",
        input_message_content=InputTextMessageContent(
            message_text=(
                "👋 Я пользуюсь ботом, который сохраняет удалённые и изменённые "
                "сообщения в Telegram.\n\n"
                f"🎁 Переходи по ссылке и забери бонус: {link}"
            ),
            disable_web_page_preview=False,
        ),
        reply_markup=InlineKeyboardMarkup(
            inline_keyboard=[
                [InlineKeyboardButton(text="🚀 Подключить бота (+3 дня)", url=link)]
            ]
        ),
    )

    await query.answer([result], cache_time=5, is_personal=True)
