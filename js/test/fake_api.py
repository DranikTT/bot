#!/usr/bin/env python3
"""Фейковый Telegram Bot API для проверки JS-бота.

Поднимает HTTP-сервер, отдаёт заранее заготовленные обновления и записывает
все вызовы, которые сделал бот. Запускается на 127.0.0.1:8099.

    python3 test/fake_api.py &   # сервер
    API_SERVER=http://127.0.0.1:8099 node bot.js
"""

from __future__ import annotations

import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(__import__("os").environ.get("FAKE_PORT", "8099"))
CALLS: list[dict] = []
UPDATES: list[dict] = []
# Отказ на кастомные эмодзи — воспроизводит боевой DOCUMENT_INVALID.
# Имитация отказа DOCUMENT_INVALID: REJECT_EMOJI=1 в окружении.
REJECT_EMOJI = {"value": __import__("os").environ.get("REJECT_EMOJI", "") == "1"}
# Чаты, которые ведут себя как Business: Telegram отказывает в удалении
# сообщения без business_connection_id. Пустое множество — обычный режим.
BUSINESS_CHATS = {"value": {"555"}}
_FAILED_DELETES = {"value": 0}
LOCK = threading.Lock()

USER = {"id": 777, "first_name": "Вася", "username": "vasya", "is_bot": False}
CHAT = {"id": 777, "type": "private", "first_name": "Вася", "username": "vasya"}


def update_message(update_id: int, text: str, message_id: int = 1) -> dict:
    return {
        "update_id": update_id,
        "message": {
            "message_id": message_id,
            "from": USER,
            "chat": CHAT,
            "date": 1700000000,
            "text": text,
        },
    }


def update_callback(update_id: int, data: str, message_id: int = 50) -> dict:
    return {
        "update_id": update_id,
        "callback_query": {
            "id": f"cb{update_id}",
            "from": USER,
            "message": {"message_id": message_id, "chat": CHAT, "date": 1700000000},
            "chat_instance": "1",
            "data": data,
        },
    }


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args) -> None:
        pass

    def _read_chunked(self) -> bytes:
        chunks = []
        while True:
            line = self.rfile.readline().strip()
            if not line:
                break
            size = int(line.split(b";")[0], 16)
            if size == 0:
                self.rfile.readline()
                break
            chunks.append(self.rfile.read(size))
            self.rfile.read(2)
        return b"".join(chunks)

    def _payload(self) -> dict:
        if (self.headers.get("Transfer-Encoding") or "").lower() == "chunked":
            raw = self._read_chunked()
        else:
            length = int(self.headers.get("Content-Length") or 0)
            raw = self.rfile.read(length) if length else b""

        content_type = self.headers.get("Content-Type") or ""

        if "multipart/form-data" in content_type:
            # вытаскиваем текстовые поля и имя файла из multipart
            boundary = content_type.split("boundary=")[-1].strip().strip('"')
            fields: dict = {}
            for part in raw.split(f"--{boundary}".encode()):
                head, _, body = part.partition(b"\r\n\r\n")
                head_text = head.decode("utf-8", "replace")
                if 'name="' not in head_text:
                    continue
                name = head_text.split('name="', 1)[1].split('"', 1)[0]
                if 'filename="' in head_text:
                    fields[name] = head_text.split('filename="', 1)[1].split('"', 1)[0]
                    continue
                value = body.rstrip(b"--").strip(b"\r\n").decode("utf-8", "replace")
                if value.startswith(("{", "[")):
                    try:
                        fields[name] = json.loads(value)
                        continue
                    except json.JSONDecodeError:
                        pass
                fields[name] = value
            return fields

        try:
            return json.loads(raw or b"{}")
        except json.JSONDecodeError:
            return {}

    def do_POST(self) -> None:
        # Проверяем ПУТЬ ЦЕЛИКОМ, как настоящий Telegram: /bot<token>/<method>.
        # Раньше метод брался как последний сегмент, и двойной /bot/bot<token>/
        # проходил незамеченным — боевой 404 на getMe тесты не ловили.
        path = self.path.split("?")[0]
        parts = [part for part in path.split("/") if part]

        if len(parts) < 2 or not parts[0].startswith("bot") or len(parts[0]) < 5:
            return self._send({
                "ok": False,
                "error_code": 404,
                "description": "Not Found",
            }, status=404)

        if parts[0] == "bot":
            # ровно то, что случилось в проде: /bot/bot<token>/getMe
            return self._send({
                "ok": False,
                "error_code": 404,
                "description": "Not Found",
            }, status=404)

        method = parts[-1]
        payload = self._payload()

        with LOCK:
            if method == "getUpdates":
                offset = int(payload.get("offset") or 0)
                pending = [u for u in UPDATES if u["update_id"] >= offset]
                UPDATES.clear()
                return self._send({"ok": True, "result": pending})

            if method == "getMe":
                return self._send({"ok": True, "result": {
                    "id": 42, "is_bot": True, "first_name": "Test",
                    "username": "test_bot", "can_join_groups": True,
                }})

            CALLS.append({"method": method, "payload": payload})

            # Так ведёт себя настоящий Telegram в Business-чате:
            # deleteMessage без business_connection_id не удаляет сообщение.
            # Бот молча терял удаление, и меню копились стопкой.
            if method == "deleteMessage" and not payload.get("business_connection_id"):
                chat_id = payload.get("chat_id")
                # только для чатов из списка Business: в обычном чате удаление работает
                if chat_id is not None and str(chat_id) in BUSINESS_CHATS["value"]:
                    CALLS[-1]["failed"] = True
                    _FAILED_DELETES["value"] += 1
                    return self._send({
                        "ok": False,
                        "error_code": 400,
                        "description": "Bad Request: message can't be deleted",
                    })

            body_text = json.dumps(payload, ensure_ascii=False)
            has_custom_emoji = (
                "tg-emoji" in body_text
                or "icon_custom_emoji_id" in body_text
            )

            if REJECT_EMOJI["value"] and has_custom_emoji:
                return self._send({
                    "ok": False,
                    "error_code": 400,
                    "description": "Bad Request: DOCUMENT_INVALID",
                })

            if method in {"sendMessage", "sendPhoto", "editMessageText", "sendInvoice"}:
                return self._send({"ok": True, "result": {
                    "message_id": 100, "date": 1700000000, "chat": CHAT,
                    "text": payload.get("text", ""),
                }})

            return self._send({"ok": True, "result": True})

    def _send(self, body: dict, status: int = 200) -> None:
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def update_business_connection(update_id: int, connection_id: str, enabled: bool) -> dict:
    return {
        "update_id": update_id,
        "business_connection": {
            "id": connection_id,
            "user": USER,
            "user_chat_id": USER["id"],
            "date": 1700000000,
            "is_enabled": enabled,
        },
    }


def update_business_callback(update_id: int, data: str, connection_id: str) -> dict:
    """Нажатие кнопки в Business-чате с клиентом (chat_id ≠ владелец)."""
    client = {"id": 555, "first_name": "Клиент", "is_bot": False}
    return {
        "update_id": update_id,
        "callback_query": {
            "id": f"cb{update_id}",
            "from": USER,
            "message": {
                "message_id": 60,
                "chat": {"id": 555, "type": "private", "first_name": "Клиент"},
                "date": 1700000000,
                "business_connection_id": connection_id,
            },
            "chat_instance": "1",
            "data": data,
        },
    }


def scenario() -> list[dict]:
    """Сценарий: старт → настройки → тарифы → премиум → реферал → архив."""
    return [
        update_message(1, "/start"),
        update_callback(2, "menu_settings"),
        update_callback(3, "toggle_deleted"),
        update_callback(4, "menu_sub"),
        update_callback(5, "menu_premium"),
        update_callback(6, "menu_ref"),
        update_callback(7, "menu_connect"),
        update_callback(8, "back_main"),
        update_callback(9, "buy_sub_30"),
        # --- Business: бот сам узнаёт о подключении, затем меню в чате клиента
        update_business_connection(10, "conn-e2e", True),
        update_business_callback(11, "back_main", "conn-e2e"),
        update_business_callback(12, "menu_settings", "conn-e2e"),
        update_business_callback(13, "menu_sub", "conn-e2e"),
    ]


def main() -> int:
    UPDATES.extend(scenario())
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()

    print(f"fake Telegram API на http://127.0.0.1:{PORT}", flush=True)

    deadline = time.time() + 12
    while time.time() < deadline:
        time.sleep(0.4)
        with LOCK:
            if len(CALLS) >= 9:
                break

    time.sleep(1.5)
    server.shutdown()

    with LOCK:
        calls = list(CALLS)

    print(json.dumps(calls, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
