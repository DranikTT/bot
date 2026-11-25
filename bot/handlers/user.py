"""Обработчики для обычных пользователей: меню, подписка, экспорт, импорт."""

from __future__ import annotations

import logging
import shutil
import tempfile
from pathlib import Path

from aiogram import Bot, F, Router
from aiogram.filters import Command, CommandObject, CommandStart
from aiogram.fsm.context import FSMContext
from aiogram.types import (
    BufferedInputFile,
    CallbackQuery,
    Message,
)

from .. import database as db
from ..archive import extract_zip, generate_html_archive, import_telegram_desktop
from ..config import (
    MAX_IMPORT_FILE_MB,
    REFERRAL_BONUS_DAYS,
    REGISTRATION_BONUS_DAYS,
    SUB_PRICES,
    TEMP_DIR,
    is_admin,
)
from ..keyboards import (
    chats_keyboard,
    empty_chats_keyboard,
    import_result_keyboard,
    import_wait_keyboard,
)
from ..menu import (
    ARCHIVE_TYPE_LABELS,
    archive_types_menu,
    connect_menu,
    create_bot_menu,
    escape_html,
    invite_menu,
    main_menu,
    premium_menu,
    settings_menu,
    show_menu,
    tariffs_menu,
    typing,
)

log = logging.getLogger(__name__)
router = Router(name="user")

DEFAULT_ARCHIVE_TYPES = set(ARCHIVE_TYPE_LABELS)


# --------------------------------------------------------------------------
# Пользователи и бонусы
# --------------------------------------------------------------------------


def get_or_create_user(user) -> dict:
    user_id = int(user.id)
    existing = db.get_user(user_id)
    if existing:
        return existing

    created, is_new, referrer_id = db.create_user(
        user_id=user_id,
        username=user.username or "",
        first_name=user.first_name or "",
    )
    if is_new and referrer_id:
        return db.get_user(user_id) or created
    return created


def ensure_user(user) -> dict:
    """Гарантирует, что пользователь есть в БД (создаёт при необходимости)."""
    if user is None:
        return {}
    existing = db.get_user(int(user.id))
    if existing:
        return existing
    created, _, _ = db.create_user(
        user_id=int(user.id),
        username=getattr(user, "username", "") or "",
        first_name=getattr(user, "first_name", "") or "",
    )
    return created or {}


def is_banned(user: dict | None) -> bool:
    return bool(user and int(user.get("is_banned") or 0) == 1)


async def guard(message: Message) -> dict | None:
    """Пропускает только в личку и не для забаненных."""
    if not message.from_user or message.chat.type != "private":
        return None

    user = get_or_create_user(message.from_user)
    if is_banned(user) and not is_admin(message.from_user.id):
        await message.answer("🚫 Ваш доступ к боту заблокирован.")
        return None
    return user


# --------------------------------------------------------------------------
# /start
# --------------------------------------------------------------------------


@router.message(CommandStart())
async def cmd_start(message: Message, bot: Bot) -> None:
    if not message.from_user or message.chat.type != "private":
        return

    payload = ""
    parts = (message.text or "").split(maxsplit=1)
    if len(parts) > 1:
        payload = parts[1].strip()

    user_id = int(message.from_user.id)
    existing = db.get_user(user_id)
    referred_by = None

    if not existing and payload.startswith("ref_"):
        candidate = payload.removeprefix("ref_")
        if candidate.isdigit():
            referrer = db.get_user(int(candidate))
            if referrer and int(referrer.get("is_banned") or 0) != 1 and int(candidate) != user_id:
                referred_by = int(candidate)

    if existing:
        user = existing
    else:
        user, is_new, referrer_id = db.create_user(
            user_id=user_id,
            username=message.from_user.username or "",
            first_name=message.from_user.first_name or "",
            referred_by=referred_by,
        )
        if is_new and referrer_id:
            try:
                await bot.send_message(
                    referrer_id,
                    f"🎉 По вашей ссылке зарегистрировался <b>{escape_html(message.from_user.first_name)}</b>!\n"
                    f"✅ Начислено <b>+{REFERRAL_BONUS_DAYS} дней</b> подписки.",
                    parse_mode="HTML",
                )
            except Exception:  # noqa: BLE001 — реферрер мог заблокировать бота
                pass

            await message.answer(
                f"🎁 Вы перешли по приглашению и получили <b>+{REGISTRATION_BONUS_DAYS} дня</b> подписки!",
                parse_mode="HTML",
            )

    if is_banned(user) and not is_admin(user_id):
        await message.answer("🚫 Ваш доступ к боту заблокирован.")
        return

    await show_menu(bot, message.chat.id, main_menu(user, user_id=user_id))


# --------------------------------------------------------------------------
# Команды
# --------------------------------------------------------------------------


@router.message(Command("menu"))
async def cmd_menu(message: Message, bot: Bot) -> None:
    user = await guard(message)
    if user:
        await show_menu(
            bot,
            message.chat.id,
            main_menu(user, user_id=message.from_user.id),
        )


@router.message(Command("export"))
async def cmd_export(message: Message, bot: Bot, command: CommandObject) -> None:
    user = await guard(message)
    if not user:
        return

    user_id = int(message.from_user.id)
    chats = _available_chats(user_id)

    if command.args and command.args.strip():
        target = command.args.strip()
        match = next((chat for chat in chats if str(chat["chat_id"]) == target), None)
        if not match:
            await message.answer(
                "Этот чат не найден. Используйте /export без аргументов, "
                "чтобы выбрать из списка."
            )
            return
        await _start_export(bot, message.chat.id, user_id, match)
        return

    if not chats:
        await message.answer(
            "📁 <b>Список чатов пуст</b>\n\n"
            "В базе пока нет переписок. Подключите бота в Telegram Business "
            "или импортируйте историю через /archive.",
            reply_markup=empty_chats_keyboard(),
            parse_mode="HTML",
        )
        return

    await message.answer(
        f"📁 <b>Выберите чат для экспорта</b>\n\nНайдено диалогов: <b>{len(chats)}</b>",
        reply_markup=chats_keyboard(chats),
        parse_mode="HTML",
    )


@router.message(Command("archive"))
async def cmd_archive(message: Message, state: FSMContext) -> None:
    user = await guard(message)
    if not user:
        return

    await state.set_state("archive_wait_file")
    await message.answer(
        "📥 <b>Импорт истории Telegram Desktop</b>\n\n"
        "Отправьте файл <code>result.json</code> или ZIP-архив экспорта.",
        reply_markup=import_wait_keyboard(),
        parse_mode="HTML",
    )


@router.message(Command("connect"))
async def cmd_connect(message: Message, bot: Bot) -> None:
    user = await guard(message)
    if not user:
        return
    username = (await bot.me()).username or ""
    await show_menu(bot, message.chat.id, connect_menu(username))


# --------------------------------------------------------------------------
# Кнопки главного меню
# --------------------------------------------------------------------------


@router.callback_query(F.data == "back_main")
async def on_back_main(callback: CallbackQuery, bot: Bot, state: FSMContext) -> None:
    await state.clear()
    user = ensure_user(callback.from_user)
    await show_menu(
        bot,
        callback.message.chat.id,
        main_menu(user, user_id=callback.from_user.id),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.callback_query(F.data == "settings")
async def on_settings(callback: CallbackQuery, bot: Bot, state: FSMContext) -> None:
    await state.clear()
    await show_menu(
        bot,
        callback.message.chat.id,
        settings_menu(callback.from_user.id),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.callback_query(F.data.in_({"toggle_deleted", "toggle_edited"}))
async def on_toggle_notify(callback: CallbackQuery, bot: Bot) -> None:
    field = "notify_deleted" if callback.data == "toggle_deleted" else "notify_edited"
    user = ensure_user(callback.from_user)
    current = int(user.get(field, 1) or 0)
    db.set_flag(callback.from_user.id, field, not current)

    await show_menu(
        bot,
        callback.message.chat.id,
        settings_menu(callback.from_user.id),
        message_id=callback.message.message_id,
    )
    label = "Удалённые" if field == "notify_deleted" else "Изменённые"
    await callback.answer(f"{label}: {'ВКЛ ✅' if not current else 'ВЫКЛ ❌'}")


@router.callback_query(F.data == "connect")
async def on_connect(callback: CallbackQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    await show_menu(
        bot,
        callback.message.chat.id,
        connect_menu(username),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.callback_query(F.data == "connect_help")
async def on_connect_help(callback: CallbackQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    text = (
        "📌 <b>Как подключить бота</b>\n\n"
        "1. Telegram → <b>Настройки</b> → <b>Telegram Business</b>\n"
        "2. <b>Чат-боты</b> → <b>Добавить бота</b>\n"
        f"3. Введите <code>@{escape_html(username)}</code>\n"
        "4. Разрешите чтение сообщений и ответы\n\n"
        "Готово — бот начнёт сохранять переписку и присылать уведомления."
    )
    await callback.message.answer(text, parse_mode="HTML")
    await callback.answer()


@router.callback_query(F.data == "check_connection")
async def on_check_connection(callback: CallbackQuery, bot: Bot) -> None:
    user_id = callback.from_user.id
    connection = db.get_business_connection(user_id)

    if connection and int(connection.get("is_enabled") or 0) == 1:
        await callback.answer("✅ Подключение активно", show_alert=True)
        user = ensure_user(callback.from_user)
        await show_menu(
            bot,
            callback.message.chat.id,
            main_menu(user, user_id=user_id),
            message_id=callback.message.message_id,
        )
        return

    await callback.answer("❌ Подключение не найдено", show_alert=True)


@router.callback_query(F.data == "create_bot")
async def on_create_bot(callback: CallbackQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    await show_menu(
        bot,
        callback.message.chat.id,
        create_bot_menu(username),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.callback_query(F.data == "open_botfather")
async def on_open_botfather(callback: CallbackQuery) -> None:
    await callback.answer(
        "Откройте https://t.me/BotFather и отправьте /newbot",
        show_alert=True,
    )


@router.callback_query(F.data == "create_bot_check")
async def on_create_bot_check(callback: CallbackQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    user_id = callback.from_user.id
    connection = db.get_business_connection(user_id)
    connected = bool(connection and int(connection.get("is_enabled") or 0) == 1)

    await callback.answer(
        "✅ Бот подключён" if connected else f"Бот @{username} ещё не подключён к Business",
        show_alert=True,
    )


# --------------------------------------------------------------------------
# Тарифы, премиум, оплата
# --------------------------------------------------------------------------


@router.callback_query(F.data == "tariffs")
async def on_tariffs(callback: CallbackQuery, bot: Bot) -> None:
    await show_menu(
        bot,
        callback.message.chat.id,
        tariffs_menu(callback.from_user.id),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.callback_query(F.data == "premium")
async def on_premium(callback: CallbackQuery, bot: Bot) -> None:
    await show_menu(
        bot,
        callback.message.chat.id,
        premium_menu(callback.from_user.id),
        message_id=callback.message.message_id,
    )
    await callback.answer()


INVOICES = {
    "buy_sub_7": ("Подписка на 7 дней", "Доступ ко всем функциям на 7 дней.", "sub_7", "days7"),
    "buy_sub_30": ("Подписка на 30 дней", "Доступ ко всем функциям на 30 дней.", "sub_30", "days30"),
    "buy_sub_90": ("Подписка на 90 дней", "Доступ ко всем функциям на 90 дней.", "sub_90", "days90"),
    "buy_sub_premium": ("Премиум на 365 дней", "Премиум-тариф и 365 дней подписки.", "sub_premium", "premium"),
}


@router.callback_query(F.data.in_(INVOICES.keys()))
async def on_buy(callback: CallbackQuery, bot: Bot) -> None:
    title, description, payload, price_key = INVOICES[callback.data]
    amount = SUB_PRICES[price_key]

    try:
        await bot.send_invoice(
            chat_id=callback.message.chat.id,
            title=title,
            description=description,
            payload=f"{payload}_{callback.from_user.id}",
            provider_token="",
            currency="XTR",
            prices=[{"label": title, "amount": amount}],
        )
        await callback.answer()
    except Exception as error:  # noqa: BLE001
        log.exception("[Invoice] %s", error)
        await callback.answer("Не удалось выставить счёт. Попробуйте позже.", show_alert=True)


# --------------------------------------------------------------------------
# Рефералы
# --------------------------------------------------------------------------


@router.callback_query(F.data == "invite")
async def on_invite(callback: CallbackQuery, bot: Bot) -> None:
    user = ensure_user(callback.from_user)
    username = (await bot.me()).username or ""
    await show_menu(
        bot,
        callback.message.chat.id,
        invite_menu(user, username),
        message_id=callback.message.message_id,
    )
    await callback.answer()


@router.callback_query(F.data == "share_link")
async def on_share_link(callback: CallbackQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    link = f"https://t.me/{username}?start=ref_{callback.from_user.id}"
    share = f"https://t.me/share/url?url={link}&text=Забирай+бонус"
    await callback.message.answer(
        f"🔗 Ваша ссылка:\n<code>{escape_html(link)}</code>\n\n"
        f"Переслать другу: {share}",
        parse_mode="HTML",
        disable_web_page_preview=True,
    )
    await callback.answer()


@router.callback_query(F.data == "invite_url")
async def on_invite_url(callback: CallbackQuery, bot: Bot) -> None:
    username = (await bot.me()).username or ""
    link = f"https://t.me/{username}?start=ref_{callback.from_user.id}"
    await callback.answer(f"Ссылка: {link}", show_alert=True)


# --------------------------------------------------------------------------
# Экспорт и импорт
# --------------------------------------------------------------------------


def _available_chats(user_id: int) -> list[dict]:
    chats = []
    for row in db.get_owner_chats(user_id):
        chat_id = str(row.get("chat_id"))
        title = (
            row.get("peer_name")
            or row.get("peer_username")
            or f"Чат {chat_id}"
        )
        if row.get("peer_username") and row.get("peer_name"):
            title = f"{row['peer_name']} (@{row['peer_username']})"
        chats.append({"chat_id": chat_id, "title": title, "count": row.get("msg_count", 0)})
    return chats


async def _start_export(bot: Bot, chat_id: int, user_id: int, target: dict) -> None:
    state_types = set(DEFAULT_ARCHIVE_TYPES)
    menu = archive_types_menu(target["chat_id"], target["title"], state_types)
    # Типы храним в памяти процесса — как и раньше
    from ..runtime import archive_sessions

    archive_sessions[user_id] = {
        "chat_id": target["chat_id"],
        "title": target["title"],
        "types": state_types,
    }
    await show_menu(bot, chat_id, menu)


@router.callback_query(F.data == "export_chats")
async def on_export_chats(callback: CallbackQuery, bot: Bot) -> None:
    chats = _available_chats(callback.from_user.id)
    if not chats:
        await callback.message.edit_text(
            "📁 <b>Список чатов пуст</b>\n\nПодключите бота в Telegram Business "
            "или импортируйте историю через /archive.",
            reply_markup=empty_chats_keyboard(),
            parse_mode="HTML",
        )
        await callback.answer()
        return

    await callback.message.edit_text(
        f"📁 <b>Выберите чат</b>\n\nНайдено: <b>{len(chats)}</b>",
        reply_markup=chats_keyboard(chats),
        parse_mode="HTML",
    )
    await callback.answer()


@router.callback_query(F.data.startswith("arch_sel_"))
async def on_select_chat(callback: CallbackQuery, bot: Bot) -> None:
    target_id = callback.data.removeprefix("arch_sel_")
    target = next(
        (chat for chat in _available_chats(callback.from_user.id) if chat["chat_id"] == target_id),
        None,
    )
    if not target:
        await callback.answer("Чат не найден", show_alert=True)
        return

    await callback.message.delete()
    await _start_export(bot, callback.message.chat.id, callback.from_user.id, target)
    await callback.answer()


@router.callback_query(F.data.startswith("arch_toggle_"))
async def on_toggle_type(callback: CallbackQuery, bot: Bot) -> None:
    from ..runtime import archive_sessions

    session = archive_sessions.get(callback.from_user.id)
    if not session:
        await callback.answer("Сессия истекла", show_alert=True)
        return

    kind = callback.data.removeprefix("arch_toggle_")
    if kind in session["types"]:
        session["types"].discard(kind)
    else:
        session["types"].add(kind)

    menu = archive_types_menu(session["chat_id"], session["title"], session["types"])
    await show_menu(bot, callback.message.chat.id, menu, message_id=callback.message.message_id)
    await callback.answer()


@router.callback_query(F.data == "arch_cancel")
async def on_arch_cancel(callback: CallbackQuery, bot: Bot) -> None:
    from ..runtime import archive_sessions

    archive_sessions.pop(callback.from_user.id, None)
    user = ensure_user(callback.from_user)
    await show_menu(
        bot,
        callback.message.chat.id,
        main_menu(user, user_id=callback.from_user.id),
        message_id=callback.message.message_id,
    )
    await callback.answer("Отменено")


@router.callback_query(F.data == "arch_start")
async def on_arch_start(callback: CallbackQuery, bot: Bot) -> None:
    from ..runtime import archive_sessions

    session = archive_sessions.get(callback.from_user.id)
    if not session:
        await callback.answer("Сессия истекла", show_alert=True)
        return
    if not session["types"]:
        await callback.answer("Выберите хотя бы один тип", show_alert=True)
        return

    await callback.answer("Собираю архив…")
    await typing(bot, callback.message.chat.id)

    user_id = callback.from_user.id
    try:
        html, count = generate_html_archive(
            owner_id=user_id,
            chat_id=session["chat_id"],
            chat_title=session["title"],
            allowed_types=session["types"],
        )
    except Exception as error:  # noqa: BLE001
        log.exception("[Archive] Ошибка генерации: %s", error)
        await callback.message.answer(f"❌ Ошибка: <code>{escape_html(error)}</code>", parse_mode="HTML")
        return

    if count == 0:
        await callback.message.answer("В этом чате нет сообщений выбранных типов.")
        return

    document = BufferedInputFile(
        html.encode("utf-8"),
        filename=f"chat_{session['chat_id']}.html",
    )
    await bot.send_document(
        chat_id=callback.message.chat.id,
        document=document,
        caption=f"📦 <b>HTML-архив готов</b>\nСообщений: <b>{count}</b>",
        parse_mode="HTML",
    )

    archive_sessions.pop(user_id, None)
    user = ensure_user(callback.from_user)
    await show_menu(
        bot,
        callback.message.chat.id,
        main_menu(user, user_id=user_id),
        message_id=callback.message.message_id,
    )


@router.callback_query(F.data == "archive_import_cancel")
async def on_import_cancel(callback: CallbackQuery, bot: Bot, state: FSMContext) -> None:
    await state.clear()
    user = ensure_user(callback.from_user)
    await show_menu(
        bot,
        callback.message.chat.id,
        main_menu(user, user_id=callback.from_user.id),
        message_id=callback.message.message_id,
    )
    await callback.answer("Импорт отменён")


@router.callback_query(F.data == "archive_import_again")
async def on_import_again(callback: CallbackQuery, state: FSMContext) -> None:
    await state.set_state("archive_wait_file")
    await callback.message.edit_text(
        "📥 Отправьте <code>result.json</code> или ZIP-архив.",
        reply_markup=import_wait_keyboard(),
        parse_mode="HTML",
    )
    await callback.answer()


@router.message(F.document, F.chat.type == "private")
async def on_document(message: Message, bot: Bot, state: FSMContext) -> None:
    current_state = await state.get_state()
    document = message.document
    file_name = (document.file_name or "").lower()

    is_archive = file_name.endswith((".json", ".zip"))

    if current_state != "archive_wait_file":
        if is_archive:
            await message.answer("Для импорта сначала отправьте /archive, затем файл.")
        return

    if not is_archive:
        await message.answer("Нужен файл <code>result.json</code> или ZIP-архив.", parse_mode="HTML")
        return

    max_bytes = int(MAX_IMPORT_FILE_MB * 1024 * 1024)
    if (document.file_size or 0) > max_bytes:
        await state.clear()
        await message.answer(f"Файл больше {int(MAX_IMPORT_FILE_MB)} МБ.")
        return

    await state.clear()
    status = await message.answer("📥 Скачиваю и импортирую…")

    workdir = Path(tempfile.mkdtemp(dir=TEMP_DIR, prefix="import_"))
    try:
        destination = workdir / ("import.zip" if file_name.endswith(".zip") else "result.json")
        await bot.download(document, destination=destination)

        json_path = extract_zip(destination, workdir / "extracted") if file_name.endswith(".zip") else destination
        imported = import_telegram_desktop(json_path, int(message.from_user.id))

        await status.edit_text(
            f"✅ <b>Импорт завершён</b>\n\n• Добавлено сообщений: <b>{imported}</b>\n\n"
            "Для архива используйте /export.",
            reply_markup=import_result_keyboard(True),
            parse_mode="HTML",
        )
    except Exception as error:  # noqa: BLE001
        log.exception("[Import] %s", error)
        await status.edit_text(
            f"❌ <b>Ошибка импорта:</b> <code>{escape_html(error)}</code>",
            reply_markup=import_result_keyboard(False),
            parse_mode="HTML",
        )
    finally:
        shutil.rmtree(workdir, ignore_errors=True)
