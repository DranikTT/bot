"""Админ-панель: подписки, баны, проверка юзеров, рассылка."""

from __future__ import annotations

import asyncio
import logging

from aiogram import Bot, F, Router
from aiogram.filters import Command
from aiogram.fsm.context import FSMContext
from aiogram.types import CallbackQuery, Message

from .. import database as db
from ..config import ADMIN_IDS, is_admin
from ..keyboards import admin_back_keyboard
from ..menu import admin_menu, format_sub_status, format_user_nick, show_menu
from ..states import AdminFlow

log = logging.getLogger(__name__)
router = Router(name="admin")
router.message.filter(F.chat.type == "private")
router.callback_query.filter(F.from_user.id.in_(ADMIN_IDS))


async def _admin_gate(message: Message) -> bool:
    if not message.from_user or not is_admin(message.from_user.id):
        await message.answer("⛔️ У вас нет прав администратора.")
        return False
    return True


@router.message(Command("admin"))
async def cmd_admin(message: Message, bot: Bot) -> None:
    if not await _admin_gate(message):
        return
    await show_menu(bot, message.chat.id, admin_menu())


@router.callback_query(F.data == "admin")
async def on_admin_menu(callback: CallbackQuery, bot: Bot, state: FSMContext) -> None:
    await state.clear()
    await show_menu(
        bot,
        callback.message.chat.id,
        admin_menu(),
        message_id=callback.message.message_id,
    )
    await callback.answer()


PROMTS = {
    "admin_give_sub": ("give_sub", "➕ <b>Выдача подписки</b>\n\nОтправьте ID и количество дней через пробел: <code>123456789 30</code>"),
    "admin_take_sub": ("take_sub", "➖ <b>Аннулирование подписки</b>\n\nОтправьте Telegram ID:"),
    "admin_toggle_prem": ("toggle_prem", "⭐️ <b>Выдать / снять Премиум</b>\n\nОтправьте Telegram ID:"),
    "admin_ban_user": ("ban_user", "🔨 <b>Бан</b>\n\nОтправьте Telegram ID:"),
    "admin_unban_user": ("unban_user", "🔓 <b>Разбан</b>\n\nОтправьте Telegram ID:"),
    "admin_check_user": ("check_user", "🔍 <b>Проверка юзера</b>\n\nОтправьте Telegram ID:"),
    "admin_broadcast": ("broadcast", "📢 <b>Рассылка</b>\n\nОтправьте текст — он уйдёт всем пользователям."),
}


@router.callback_query(F.data.in_(PROMTS.keys()))
async def on_admin_action(callback: CallbackQuery, bot: Bot, state: FSMContext) -> None:
    action, text = PROMTS[callback.data]
    await state.set_state(AdminFlow.wait_user_id)
    await state.update_data(action=action)

    await callback.message.edit_text(
        text,
        reply_markup=admin_back_keyboard(),
        parse_mode="HTML",
    )
    await callback.answer()


@router.callback_query(F.data == "admin_back")
async def on_admin_back(callback: CallbackQuery, bot: Bot, state: FSMContext) -> None:
    await state.clear()
    await show_menu(
        bot,
        callback.message.chat.id,
        admin_menu(),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.message(AdminFlow.wait_user_id)
async def on_admin_input(message: Message, bot: Bot, state: FSMContext) -> None:
    if not await _admin_gate(message):
        return

    data = await state.get_data()
    action = data.get("action")
    text = (message.text or "").strip()

    if action == "give_sub":
        parts = text.split()
        if len(parts) < 1 or not parts[0].isdigit():
            await message.answer("Формат: <code>123456789 30</code>", parse_mode="HTML")
            return
        target_id = int(parts[0])
        days = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 30
        db.create_user(target_id)
        db.add_subscription_days(target_id, days)
        await state.clear()
        await message.answer(
            f"✅ Пользователю <code>{target_id}</code> начислено <b>{days} дн.</b>",
            reply_markup=admin_back_keyboard(),
            parse_mode="HTML",
        )
        try:
            await bot.send_message(target_id, f"💎 Вам выдана подписка на <b>{days} дн.</b>", parse_mode="HTML")
        except Exception:  # noqa: BLE001
            pass
        return

    if action == "broadcast":
        await state.clear()
        await _broadcast(bot, message, text)
        return

    if not text.isdigit():
        await message.answer("Введите числовой Telegram ID.")
        return

    target_id = int(text)
    if action == "toggle_prem":
        db.create_user(target_id)
        user = db.get_user(target_id) or {}
        next_state = int(user.get("is_premium") or 0) != 1
        db.set_premium(target_id, next_state)
        result = f"Премиум {'выдан' if next_state else 'снят'}"
    elif action == "ban_user":
        db.set_ban(target_id, True)
        result = "пользователь забанен"
    elif action == "unban_user":
        db.set_ban(target_id, False)
        result = "пользователь разбанен"
    elif action == "take_sub":
        db.reset_subscription(target_id)
        result = "подписка аннулирована"
    elif action == "check_user":
        user = db.get_user(target_id)
        await state.clear()
        if not user:
            await message.answer(
                f"❌ Пользователь <code>{target_id}</code> не найден.",
                reply_markup=admin_back_keyboard(),
                parse_mode="HTML",
            )
            return

        connection = db.get_business_connection(target_id)
        connected = bool(connection and int(connection.get("is_enabled") or 0) == 1)
        await message.answer(
            f"👤 <b>Досье</b> <code>{target_id}</code>\n\n"
            f"• Имя: <b>{format_user_nick(user)}</b>\n"
            f"• Подписка: <b>{format_sub_status(user)}</b>\n"
            f"• Премиум: <b>{'да ⭐️' if int(user.get('is_premium') or 0) else 'нет'}</b>\n"
            f"• Бан: <b>{'да 🚫' if int(user.get('is_banned') or 0) else 'нет'}</b>\n"
            f"• Рефералов: <b>{int(user.get('referrals_count') or 0)}</b>\n"
            f"• Business: <b>{'подключён ✅' if connected else 'нет ❌'}</b>",
            reply_markup=admin_back_keyboard(),
            parse_mode="HTML",
        )
        return
    else:
        await state.clear()
        await message.answer("Неизвестное действие.", reply_markup=admin_back_keyboard())
        return

    await state.clear()
    await message.answer(
        f"✅ <code>{target_id}</code>: {result}",
        reply_markup=admin_back_keyboard(),
        parse_mode="HTML",
    )


async def _broadcast(bot: Bot, message: Message, text: str) -> None:
    users = db.get_all_users()
    status = await message.answer(f"📢 Рассылка по <b>{len(users)}</b> пользователям…", parse_mode="HTML")

    sent = failed = 0
    for user in users:
        target_id = user.get("user_id")
        if not target_id:
            continue
        try:
            if text:
                await bot.send_message(target_id, text)
            else:
                await bot.copy_message(target_id, message.chat.id, message.message_id)
            sent += 1
        except Exception:  # noqa: BLE001
            failed += 1
        # Telegram ограничивает ~30 сообщений в секунду
        await asyncio.sleep(0.04)

    await status.edit_text(
        f"✅ <b>Рассылка завершена</b>\n\n• Доставлено: <b>{sent}</b>\n• Ошибок: <b>{failed}</b>",
        reply_markup=admin_back_keyboard(),
        parse_mode="HTML",
    )
