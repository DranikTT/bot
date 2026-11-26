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
REJECT_EMOJI = {"value": False}  # True — имитация отказа DOCUMENT_INVALID
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
        method = self.path.rsplit("/", 1)[-1]
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

    def _send(self, body: dict) -> None:
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


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
