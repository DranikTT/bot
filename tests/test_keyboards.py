"""Проверка всех клавиатур: каждая кнопка должна иметь рабочее действие."""

from __future__ import annotations

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

os.environ.setdefault("BOT_TOKEN", "123456:TEST-token")
os.environ.setdefault("ADMIN_IDS", "7")
os.environ["DATABASE_PATH"] = str(ROOT / "tests" / "test.sqlite")

from bot import database as db
from bot import keyboards as k
from bot import menu as m

db.init_db()

keyboards = {
    "main (админ)": k.main_menu_keyboard(7),
    "main (юзер)": k.main_menu_keyboard(2),
    "settings": k.settings_keyboard(7),
    "connect": k.connect_keyboard(),
    "tariffs": k.tariffs_keyboard(),
    "premium": k.premium_keyboard(),
    "invite": k.invite_keyboard("test_bot"),
    "create_bot": k.create_bot_keyboard("test_bot"),
    "archive_types": k.archive_types_keyboard({"text", "photo"}, m.ARCHIVE_TYPE_LABELS),
    "admin": k.admin_keyboard(),
    "import_ok": k.import_result_keyboard(True),
    "import_fail": k.import_result_keyboard(False),
    "chats": k.chats_keyboard([{"chat_id": "1", "title": "Тест"}]),
    "empty_chats": k.empty_chats_keyboard(),
    "admin_back": k.admin_back_keyboard(),
    "import_wait": k.import_wait_keyboard(),
}

ACTIONS = ("callback_data", "url", "switch_inline_query", "web_app", "login_url")

problems = []
total = 0

for name, kb in keyboards.items():
    buttons = [b for row in kb.inline_keyboard for b in row]
    total += len(buttons)

    for button in buttons:
        data = button.callback_data
        dump = button.model_dump(exclude_none=True)

        if not any(field in dump for field in ACTIONS):
            problems.append(f"{name}: «{button.text}» без действия")

        if data:
            if data == button.text:
                problems.append(f"{name}: «{button.text}» data == text")
            if len(data.encode()) > 64:
                problems.append(f"{name}: «{button.text}» data > 64 байт")
            if not all(ch.isalnum() or ch in "_:-." for ch in data):
                problems.append(f"{name}: «{button.text}» странный data {data!r}")

    print(f"  {'✅' if not any(name in p for p in problems) else '❌'} {name:<16} кнопок: {len(buttons)}")

print(f"\nвсего кнопок: {total}")
if problems:
    print("ПРОБЛЕМЫ:")
    for problem in problems:
        print("  ❌", problem)
    sys.exit(1)

print("Все клавиатуры валидны ✅")
