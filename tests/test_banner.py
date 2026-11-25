"""Проверка баннеров: меню обязано отправляться даже без картинки.

Именно этот сценарий давал «[sendMenu Normal Photo Error]: ... DOCUMENT_INVALID».
"""

from __future__ import annotations

import asyncio
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

os.environ.setdefault("BOT_TOKEN", "123456:TEST-token")
os.environ.setdefault("ADMIN_IDS", "7")
os.environ["DATABASE_PATH"] = str(ROOT / "tests" / "test.sqlite")

from aiogram import Bot
from aiogram.client.default import DefaultBotProperties
from aiogram.client.session.aiohttp import AiohttpSession
from aiogram.client.telegram import TelegramAPIServer
from aiogram.enums import ParseMode

import fake_telegram
from test_flow import _record_call

from bot import database as db
from bot import menu as menu_module
from bot.menu import banner_is_valid, main_menu, settings_menu, show_menu

BASE = "http://127.0.0.1:8099"


class RecordingSession(AiohttpSession):
    async def make_request(self, bot, method, timeout=None):
        return await _record_call(self, bot, method, timeout)


async def main() -> int:
    db.init_db()
    bot = Bot(
        os.environ["BOT_TOKEN"],
        session=RecordingSession(api=TelegramAPIServer.from_base(BASE)),
        default=DefaultBotProperties(parse_mode=ParseMode.HTML),
    )
    problems: list[str] = []

    print("1) Все баннеры на месте и валидны:")
    for key in menu_module.BANNERS:
        valid = banner_is_valid(key)
        print(f"   {key}: {'✅' if valid else '❌'} {menu_module.BANNERS[key].name}")
        if not valid:
            problems.append(f"баннер {key} невалиден")

    print("\n2) Меню с баннером отправляется фото:")
    before = len(fake_telegram.CALLS)
    await show_menu(bot, 7, main_menu(user_id=7))
    sent = fake_telegram.CALLS[before:]
    if not any(call["method"] == "sendPhoto" for call in sent):
        problems.append("sendPhoto не был вызван при валидном баннере")
    print("   методы:", [call["method"] for call in sent])

    print("\n3) Кэш file_id после первой отправки:")
    print("   BANNER_FILE_IDS:", menu_module.BANNER_FILE_IDS)
    if not menu_module.BANNER_FILE_IDS.get("main"):
        problems.append("file_id баннера не закэширован")

    print("\n4) Баннера нет на диске (главная причина DOCUMENT_INVALID):")
    missing = ROOT / "assets" / "no_such_banner.jpg"
    menu_module.BANNERS["main"] = missing
    print(f"   файл существует: {missing.exists()}")
    print(f"   banner_is_valid: {banner_is_valid('main')}")

    before = len(fake_telegram.CALLS)
    result = await show_menu(bot, 7, main_menu(user_id=7))
    calls = fake_telegram.CALLS[before:]
    methods = [call["method"] for call in calls]
    print("   методы:", methods)

    if "sendPhoto" in methods:
        problems.append("бот всё ещё пытается отправить несуществующий баннер")
    if "sendMessage" not in methods:
        problems.append("меню не отправлено как текст")
    if result is None:
        problems.append("меню не вернуло сообщение")

    print("\n5) Меню настроек без файла тоже должно работать:")
    menu_module.BANNERS["settings"] = ROOT / "assets" / "broken.jpg"
    before = len(fake_telegram.CALLS)
    await show_menu(bot, 7, settings_menu(7))
    methods = [call["method"] for call in fake_telegram.CALLS[before:]]
    print("   методы:", methods)
    if "sendMessage" not in methods:
        problems.append("меню настроек не отправилось текстом")

    print("\n6) Битый файл (есть, но не картинка):")
    broken = ROOT / "tests" / "broken.jpg"
    broken.write_bytes(b"this is not a jpeg")
    menu_module.BANNERS["admin"] = broken
    valid = banner_is_valid("admin")
    print(f"   banner_is_valid: {valid}")
    if valid:
        problems.append("битый файл принят за валидный баннер")
    broken.unlink(missing_ok=True)

    # Восстанавливаем пути
    from bot.config import ASSETS_DIR

    menu_module.BANNERS.update(
        {
            "main": ASSETS_DIR / "banner_main.jpg",
            "settings": ASSETS_DIR / "banner_settings.jpg",
            "admin": ASSETS_DIR / "banner_admin.jpg",
        }
    )

    await bot.session.close()

    print()
    if problems:
        print("ПРОБЛЕМЫ:")
        for problem in problems:
            print("  ❌", problem)
        return 1

    print("Все проверки баннеров пройдены ✅")
    return 0


if __name__ == "__main__":
    fake_telegram.serve()
    sys.exit(asyncio.run(main()))
