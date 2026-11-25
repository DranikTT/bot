"""Проверка экспорта HTML и импорта Telegram Desktop."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

os.environ.setdefault("BOT_TOKEN", "123456:TEST-token")
os.environ["DATABASE_PATH"] = str(ROOT / "tests" / "archive_test.sqlite")

from bot import database as db
from bot.archive import extract_zip, generate_html_archive, import_telegram_desktop, normalize_type

db.init_db()
problems = []

# 1. normalize_type
for raw, expected in [
    ("voice_message", "voice"),
    ("round_video", "video_note"),
    ("video_file", "video"),
    ("gif", "animation"),
    ("photo", "photo"),
]:
    got = normalize_type(raw)
    if got != expected:
        problems.append(f"normalize_type({raw!r}) = {got!r}, ожидалось {expected!r}")
print("1) нормализация типов:", "✅" if not problems else "❌")

# 2. Импорт result.json
sample = {
    "id": 999001,
    "name": "Тестовый чат",
    "messages": [
        {"id": 1, "type": "message", "date": "2024-01-01T10:00:00",
         "date_unixtime": "1704103200", "from": "Алиса", "from_id": "user111",
         "text": "Привет!", "text_entities": []},
        {"id": 2, "type": "message", "date": "2024-01-01T10:01:00",
         "date_unixtime": "1704103260", "from": "Боб", "from_id": "user222",
         "text": [{"type": "plain", "text": "Как "}, {"type": "plain", "text": "дела?"}]},
        {"id": 3, "type": "service", "date": "2024-01-01T10:02:00"},
        {"id": 4, "type": "message", "date": "2024-01-01T10:03:00",
         "date_unixtime": "1704103380", "from": "Алиса", "from_id": "user111",
         "text": "Смотри фото", "photo": "photos/photo_1.jpg"},
    ],
}

with tempfile.TemporaryDirectory() as tmp:
    json_path = Path(tmp) / "result.json"
    json_path.write_text(json.dumps(sample, ensure_ascii=False), encoding="utf-8")

    imported = import_telegram_desktop(json_path, owner_id=111)
    print(f"2) импорт: {'✅' if imported == 3 else '❌'} сообщений: {imported} (ожидалось 3)")
    if imported != 3:
        problems.append(f"импортировано {imported}, ожидалось 3")

    # 3. Чтение из БД (проверяем расшифровку)
    rows = db.get_chat_messages("999001", 111)
    texts = [row["text"] for row in rows]
    print(f"3) тексты расшифрованы: {'✅' if 'Привет!' in texts else '❌'} {texts}")

    if "Привет!" not in texts:
        problems.append("текст не расшифровался после импорта")
    if "Как дела?" not in texts:
        problems.append("массив text[] не склеился в строку")

    # 4. Экспорт HTML
    html, count = generate_html_archive(111, "999001", "Тестовый чат", {"text", "photo"})
    checks = {
        "HTML-документ": html.startswith("<!DOCTYPE html>"),
        "заголовок чата": "Тестовый чат" in html,
        "текст сообщения": "Привет!" in html,
        "все сообщения": count == 3,
        "нет XSS": "<script>" not in html,
    }
    print("4) экспорт:", {k: v for k, v in checks.items()})
    problems.extend(f"экспорт: {k}" for k, ok in checks.items() if not ok)

    # 5. Экранирование
    db.save_message(chat_id="999001", message_id=50, owner_id=111,
                    sender_id=222, text="<script>alert(1)</script>", date=1704103200)
    html2, _ = generate_html_archive(111, "999001", "Тест", {"text"})
    escaped = "&lt;script&gt;" in html2 and "<script>alert" not in html2
    print(f"5) экранирование HTML: {'✅' if escaped else '❌'}")
    if not escaped:
        problems.append("HTML не экранируется — XSS в архиве")

    # 6. ZIP
    zip_path = Path(tmp) / "export.zip"
    with zipfile.ZipFile(zip_path, "w") as archive:
        archive.writestr("export/result.json", json.dumps(sample, ensure_ascii=False))

    found = extract_zip(zip_path, Path(tmp) / "out")
    print(f"6) ZIP: {'✅' if found.name == 'result.json' else '❌'} найден {found.name}")
    if found.name != "result.json":
        problems.append("result.json не найден в ZIP")

    # 7. Zip-slip защита
    evil = Path(tmp) / "evil.zip"
    with zipfile.ZipFile(evil, "w") as archive:
        archive.writestr("../../escaped.txt", "взлом")
        archive.writestr("result.json", "{}")

    try:
        extract_zip(evil, Path(tmp) / "safe")
        leaked = (Path(tmp).parent / "escaped.txt").exists()
        print(f"7) zip-slip: {'✅' if not leaked else '❌'} файл вне каталога: {leaked}")
        if leaked:
            problems.append("zip-slip: файл записан вне каталога распаковки")
    except Exception as error:  # noqa: BLE001
        print(f"7) zip-slip: ✅ заблокировано ({type(error).__name__})")

print()
if problems:
    print("ПРОБЛЕМЫ:")
    for problem in problems:
        print("  ❌", problem)
    sys.exit(1)

print("Экспорт и импорт работают ✅")
