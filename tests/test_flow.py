"""Сквозной тест: реальные клики по кнопкам через настоящий aiogram.

Все запросы к Telegram перехватываются фейковым сервером, поэтому
проверяется именно поведение бота, а не заглушки хендлеров.
"""

from __future__ import annotations

import asyncio
import os
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

os.environ.setdefault("BOT_TOKEN", "123456:TEST-token")
os.environ.setdefault("ADMIN_IDS", "7")
os.environ["DATABASE_PATH"] = str(ROOT / "tests" / "test.sqlite")

from aiogram import Bot
from aiogram.client.default import DefaultBotProperties
from aiogram.client.telegram import TelegramAPIServer
from aiogram.enums import ParseMode
from aiogram.fsm.storage.memory import MemoryStorage
from aiogram import Dispatcher

from bot import database as db
from bot.business import router as business_router
from bot.handlers import register_handlers

import fake_telegram

BASE = "http://127.0.0.1:8099"

used_buttons: list[dict] = []
failures: list[str] = []


async def _record_call(session, bot, method, timeout=None):
    """Обёртка над настоящим HTTP-стеком aiogram: только записываем вызовы."""
    name = method.__api_method__
    payload = method.model_dump(exclude_none=True, warnings=False)
    fake_telegram.CALLS.append({"method": name, "payload": payload})

    markup = payload.get("reply_markup")
    if isinstance(markup, dict):
        for row in markup.get("inline_keyboard", []):
            for button in row:
                used_buttons.append(button)

    return await _real_make_request(session, bot, method, timeout)


from aiogram.client.session.aiohttp import AiohttpSession

_real_make_request = AiohttpSession.make_request


class RecordingSession(AiohttpSession):
    async def make_request(self, bot, method, timeout=None):
        return await _record_call(self, bot, method, timeout)


def click(data: str, user_id: int = 7, message_id: int = 555) -> dict:
    return {
        "update_id": int(datetime.now().timestamp() * 1000) % 2_000_000_000,
        "callback_query": {
            "id": str(datetime.now().timestamp()),
            "from": {"id": user_id, "is_bot": False, "first_name": "Тест", "username": "tester"},
            "message": {
                "message_id": message_id,
                "date": int(datetime.now().timestamp()),
                "chat": {"id": user_id, "type": "private"},
                "text": "меню",
            },
            "chat_instance": "ci",
            "data": data,
        },
    }


async def main() -> int:
    db.init_db()

    bot = Bot(
        os.environ["BOT_TOKEN"],
        session=RecordingSession(api=TelegramAPIServer.from_base(BASE)),
        default=DefaultBotProperties(parse_mode=ParseMode.HTML),
    )

    dp = Dispatcher(storage=MemoryStorage())
    dp.include_router(business_router)
    register_handlers(dp)

    clicks = [
        "settings", "toggle_deleted", "toggle_edited", "back_main",
        "connect", "connect_help", "check_connection", "back_main",
        "invite", "share_link", "invite_url", "back_main",
        "tariffs", "premium", "buy_sub_7", "back_main",
        "create_bot", "open_botfather", "create_bot_check", "back_main",
        "admin", "admin_give_sub", "admin_back",
        "admin_check_user", "admin_take_sub", "admin_toggle_prem",
        "admin_ban_user", "admin_unban_user", "back_main",
        "export_chats", "back_main",
    ]

    for data in clicks:
        before = len(fake_telegram.CALLS)
        try:
            await dp.feed_update(bot, __import__("aiogram").types.Update(**click(data)))
        except Exception as error:  # noqa: BLE001
            failures.append(f"{data}: {type(error).__name__}: {error}")
            continue

        answered = any(
            call["method"] == "answerCallbackQuery"
            for call in fake_telegram.CALLS[before:]
        )
        if not answered:
            failures.append(f"{data}: answerCallbackQuery не вызван")

    # Проверяем, что ни одна кнопка не ушла с данными, равными тексту
    broken = [
        button for button in used_buttons
        if button.get("callback_data") and button["callback_data"] == button.get("text")
    ]
    no_action = [
        button for button in used_buttons
        if not any(
            key in button
            for key in ("callback_data", "url", "switch_inline_query", "web_app")
        )
    ]

    print(f"кликов: {len(clicks)}, ошибок: {len(failures)}")
    for failure in failures:
        print("  ❌", failure)

    print(f"кнопок отправлено в Telegram: {len(used_buttons)}")
    print(f"  с data == text: {len(broken)}")
    print(f"  без действия: {len(no_action)}")

    methods = {}
    for call in fake_telegram.CALLS:
        methods[call["method"]] = methods.get(call["method"], 0) + 1
    print("вызовы API:", dict(sorted(methods.items())))

    await bot.session.close()
    return 0 if not failures and not broken and not no_action else 1


if __name__ == "__main__":
    fake_telegram.serve()
    code = asyncio.run(main())
    print("РЕЗУЛЬТАТ:", "OK ✅" if code == 0 else "ЕСТЬ ПРОБЛЕМЫ ❌")
    sys.exit(code)
