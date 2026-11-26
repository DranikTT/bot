"""Точка входа: создание бота, диспетчера и запуск поллинга."""

from __future__ import annotations

import asyncio
import logging
import sys

from aiogram import Bot, Dispatcher
from aiogram.client.default import DefaultBotProperties
from aiogram.enums import ParseMode
from aiogram.exceptions import TelegramUnauthorizedError
from aiogram.fsm.storage.memory import MemoryStorage
from aiogram.types import BotCommand

import os

from aiogram.client.session.aiohttp import AiohttpSession
from aiogram.client.telegram import TelegramAPIServer

from .config import ALLOWED_UPDATES, BOT_TOKEN
from .database import init_db
from .handlers import register_handlers
from . import business

log = logging.getLogger(__name__)

COMMANDS = [
    BotCommand(command="start", description="Главное меню"),
    BotCommand(command="menu", description="Меню"),
    BotCommand(command="export", description="HTML-архив переписки"),
    BotCommand(command="archive", description="Импорт истории (JSON/ZIP)"),
    BotCommand(command="connect", description="Подключение к Business"),
    BotCommand(command="admin", description="Админ-панель"),
]


def create_dispatcher() -> Dispatcher:
    dp = Dispatcher(storage=MemoryStorage())
    dp.include_router(business.router)
    register_handlers(dp)
    return dp


async def main() -> None:
    init_db()

    # API_SERVER позволяет поднять локальный Bot API сервер (например, для
    # файлов >20 МБ) или использовать прокси.
    api_server = os.environ.get("API_SERVER", "").strip()

    if api_server:
        session = AiohttpSession(api=TelegramAPIServer.from_base(api_server))
        bot = Bot(
            BOT_TOKEN,
            session=session,
            default=DefaultBotProperties(parse_mode=ParseMode.HTML),
        )
    else:
        bot = Bot(
            BOT_TOKEN,
            default=DefaultBotProperties(parse_mode=ParseMode.HTML),
        )
    dp = create_dispatcher()

    me = await bot.me()
    log.info("Бот @%s запущен", me.username)

    await bot.set_my_commands(COMMANDS)
    await bot.delete_webhook(drop_pending_updates=True)

    try:
        await dp.start_polling(bot, allowed_updates=ALLOWED_UPDATES)
    except TelegramUnauthorizedError as error:
        log.error("Telegram отклонил токен: %s", error)
        raise SystemExit(1) from error
    finally:
        await bot.session.close()


def run() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)-8s %(name)s: %(message)s",
        stream=sys.stdout,
    )
    try:
        asyncio.run(main())
    except (KeyboardInterrupt, SystemExit):
        log.info("Остановлено пользователем")


if __name__ == "__main__":
    run()
