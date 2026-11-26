#!/usr/bin/env python3
"""Фейковый Telegram Bot API для проверки JS-бота.

Поднимает HTTP-сервер, отдаёт заранее заготовленные обновления и записывает
все вызовы, которые сделал бот. Запускается на 127.0.0.1:8099.

    python3 test/fake_api.py &   # сервер
    API_SERVER=http://127.0.0.1:8099 node bot.js
"""

from __future__ import annotations

import json
import json as _json
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
# были ли выдачи обновлений и сколько тиков подряд очередь пуста
got_updates = {"value": False}
idle = {"value": 0}
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
                if pending:
                    got_updates["value"] = True
                UPDATES[:] = []
                return self._send({"ok": True, "result": pending})

            if method == "getBusinessConnection":
                # бот спрашивает сам, если событие не дошло
                CALLS[-1]["known"] = True
                return self._send({"ok": True, "result": {
                    "id": payload.get("business_connection_id"),
                    "user": USER,
                    "user_chat_id": USER["id"],
                    "date": 1700000000,
                    "is_enabled": True,
                }})

            if method == "getFile":
                return self._send({"ok": True, "result": {
                    "file_id": payload.get("file_id"),
                    "file_unique_id": "u1",
                    "file_size": 1024,
                    "file_path": "photos/test.jpg",
                }})

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

    def do_GET(self) -> None:
        if "/file/" in self.path:
            # 1x1 JPEG-заглушка
            raw = bytes.fromhex(
                "ffd8ffe000104a46494600010100000100010000ffdb0043000806060706050807"
                "0707070909080a0c140d0c0b0b0c1912130f141d1a1f1e1d1a1c1c20242e2720"
                "222c231c1c2837292c30313434341f27393d38323c2e333432ffc0000b080001"
                "000101011100ffc4001f0000010501010101010100000000000000000102030405"
                "060708090a0bffda0008010100003f00d2cf20ffd9"
            )
            self.send_response(200)
            self.send_header("Content-Type", "image/jpeg")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
            return

        self._send({"ok": True, "result": True})

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
        # ВАЖНО: update_id растут в том порядке, в каком события должны
        # произойти. Telegram нумерует их так же. Если поставить
        # business-события с бо́льшими id, чем нажатия меню, то
        # toggle_deleted успеет выключить уведомления раньше перехвата —
        # и проверка перехвата станет бессмысленной.
        #
        # Сам перехват проверяется отдельным прогоном (LIVE=1) в
        # flow.test.js: здесь важны меню и замена сообщений, и смешивать
        # их в одном сценарии — значит получить гонку и нестабильный тест.
        update_message(1, "/start"),
        update_business_connection(2, "conn-e2e", True),
        update_callback(10, "menu_settings"),
        update_callback(11, "toggle_deleted"),
        update_callback(12, "menu_sub"),
        update_callback(13, "menu_premium"),
        update_callback(14, "menu_ref"),
        update_callback(15, "menu_connect"),
        update_callback(16, "back_main"),
        update_callback(17, "buy_sub_30"),
        # меню в Business-чате клиента
        update_business_callback(30, "back_main", "conn-e2e"),
        update_business_callback(31, "menu_settings", "conn-e2e"),
        update_business_callback(32, "menu_sub", "conn-e2e"),
    ]


def business_message_update(update_id: int, message_id: int, text: str,
                            connection_id: str = "conn-live",
                            chat_id: int = 555, from_client: bool = True,
                            self_destruct: bool = False) -> dict:
    """Обычное (не инлайн) business-сообщение в чате с клиентом."""
    sender = ({"id": chat_id, "first_name": "Клиент", "username": "client",
               "is_bot": False} if from_client else
              {"id": 777, "first_name": "Вася", "username": "vasya", "is_bot": False})
    msg = {
        "message_id": message_id,
        "date": 1700000000,
        "chat": {"id": chat_id, "type": "private", "first_name": "Клиент"},
        "from": sender,
        "text": text,
        "business_connection_id": connection_id,
    }
    if self_destruct:
        msg["has_media_spoiler"] = True

    return {"update_id": update_id, "business_message": msg}


def business_edit_update(update_id: int, message_id: int, text: str,
                         connection_id: str = "conn-live", chat_id: int = 555) -> dict:
    return {
        "update_id": update_id,
        "edited_business_message": {
            "message_id": message_id,
            "date": 1700000000,
            "edit_date": 1700000600,
            "chat": {"id": chat_id, "type": "private", "first_name": "Клиент"},
            "from": {"id": chat_id, "first_name": "Клиент", "username": "client", "is_bot": False},
            "text": text,
            "business_connection_id": connection_id,
        },
    }


def business_delete_update(update_id: int, message_ids: list[int],
                           connection_id: str = "conn-live", chat_id: int = 555) -> dict:
    return {
        "update_id": update_id,
        "deleted_business_messages": {
            "business_connection_id": connection_id,
            "chat": {"id": chat_id, "type": "private", "first_name": "Клиент"},
            "message_ids": message_ids,
        },
    }


def business_photo_update(update_id: int, message_id: int,
                          connection_id: str = "conn-live", chat_id: int = 555) -> dict:
    """Самоудаляющееся фото: без текста, со спойлером и медиа."""
    return {
        "update_id": update_id,
        "business_message": {
            "message_id": message_id,
            "date": 1700000000,
            "chat": {"id": chat_id, "type": "private", "first_name": "Клиент"},
            "from": {"id": chat_id, "first_name": "Клиент", "username": "client", "is_bot": False},
            "photo": [
                {"file_id": "photo-small", "file_unique_id": "s1", "width": 90, "height": 90, "file_size": 500},
                {"file_id": "photo-big", "file_unique_id": "s2", "width": 800, "height": 800, "file_size": 1024},
            ],
            "has_media_spoiler": True,
            "business_connection_id": connection_id,
        },
    }


def live_scenario() -> list[dict]:
    """Переписка: сообщение → правка → удалённое → самоудаляющееся."""
    return [
        business_message_update(100, 201, "Привет, как дела?"),
        business_edit_update(101, 201, "Привет, как делишки?"),
        business_message_update(102, 202, "Секретик"),
        business_photo_update(103, 203),
        business_delete_update(104, [202]),
        business_delete_update(105, [203]),
    ]


def main() -> int:
    # LIVE=1 — проверяем перехват переписки, а не меню
    if __import__("os").environ.get("LIVE") == "1":
        UPDATES.append(update_business_connection(90, "conn-live", True))
        UPDATES.extend(live_scenario())
    else:
        UPDATES.extend(scenario())
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()

    print(f"fake Telegram API на http://127.0.0.1:{PORT}", flush=True)

    live = __import__("os").environ.get("LIVE") == "1"
    deadline = time.time() + (18 if live else 40)
    while time.time() < deadline:
        time.sleep(0.4)
        with LOCK:
            if not UPDATES:
                idle["value"] += 1
            else:
                idle["value"] = 0
            # Готово, когда бот прошёл сценарий до конца: очередь обновлений
            # пуста и он уже что-то отправил. Порог по числу вызовов не
            # годится — бот делает много служебных вызовов и срабатывает
            # раньше времени.
            # Ждём, пока очередь обновлений опустеет и бот обработает
            # последнее (даём ему секунды: уведомления о перехвате
            # отправляются уже после получения обновления).
            # Ждём, пока бот обработает очередь и пришлёт уведомления о
            # перехвате (удалённое/изменённое). Одного опустошения очереди
            # мало: бот отправляет уведомления уже после получения
            # обновления, и без этой проверки журнал обрывался на середине.
            # Ждём, пока бот пришлёт уведомления о перехвате: правку,
            # удалённый текст и удалённое фото. Раньше выходили по первому
            # же уведомлению, и фото-уведомление не успевало попасть в журнал.
            intercepted = sum(
                1
                for call in CALLS
                if "Удалённое" in _json.dumps(call.get("payload", {}), ensure_ascii=False)
                or "Изменённое" in _json.dumps(call.get("payload", {}), ensure_ascii=False)
            )

            finished = intercepted >= 3 or (not UPDATES and got_updates["value"] and idle["value"] >= 25)
            if finished:
                break

    time.sleep(4.0)
    server.shutdown()

    with LOCK:
        calls = list(CALLS)

    print(json.dumps(calls, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())