"""Фейковый Telegram Bot API для офлайн-проверки бота.

Запускается как HTTP-сервер: подменяет https://api.telegram.org целиком,
поэтому тестируется настоящий aiogram, а не моки хендлеров.
"""

from __future__ import annotations

import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CALLS: list[dict] = []
MESSAGE_ID = {"value": 100}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args) -> None:  # noqa: A003 — глушим шум
        pass

    def _read_payload(self) -> dict:
        # aiohttp может присылать chunked-тело — собираем его полностью
        if (self.headers.get("Transfer-Encoding") or "").lower() == "chunked":
            chunks = []
            while True:
                size_line = self.rfile.readline().strip()
                if not size_line:
                    break
                size = int(size_line, 16)
                if size == 0:
                    self.rfile.readline()
                    break
                chunks.append(self.rfile.read(size))
                self.rfile.read(2)
            raw = b"".join(chunks)
        else:
            length = int(self.headers.get("Content-Length") or 0)
            raw = self.rfile.read(length) if length else b""

        content_type = (self.headers.get("Content-Type") or "").lower()
        self._raw_body = raw
        if "multipart/form-data" in content_type:
            import re

            boundary = content_type.split("boundary=")[-1].strip()
            parts = raw.split(b"--" + boundary.encode())
            payload: dict = {}
            for part in parts:
                if b"\r\n\r\n" not in part:
                    continue
                head, body = part.split(b"\r\n\r\n", 1)
                head_text = head.decode("utf-8", "replace")
                name_match = re.search(r'name="([^"]+)"', head_text)
                if not name_match:
                    continue
                value = body.rstrip(b"\r\n--")
                key = name_match.group(1)
                if 'filename="' in head_text:
                    payload[key] = {"_file": len(value)}
                    continue
                text = value.decode("utf-8", "replace")
                if text.startswith(("{", "[")):
                    try:
                        payload[key] = json.loads(text)
                        continue
                    except json.JSONDecodeError:
                        pass
                payload[key] = text
            return payload

        try:
            return json.loads(raw or b"{}")
        except json.JSONDecodeError:
            return {}

    def _handle(self) -> None:
        method = self.path.rsplit("/", 1)[-1]
        payload = self._read_payload()
        CALLS.append({"method": method, "payload": payload})

        if method == "getMe":
            result = {"id": 42, "is_bot": True, "first_name": "Test", "username": "test_bot"}
        elif method in {"setMyCommands", "deleteWebhook", "answerCallbackQuery", "answerPreCheckoutQuery"}:
            result = True
        elif method == "getUpdates":
            result = []
        elif method in {"sendMessage", "editMessageText"}:
            result = self._message(payload, text=True)
        elif method in {"sendPhoto", "editMessageMedia"}:
            result = self._message(payload, photo=True)
        elif method == "sendDocument":
            result = self._message(payload, document=True)
        elif method == "sendInvoice":
            result = self._message(payload)
        elif method == "getFile":
            result = {"file_id": "f", "file_path": "x.jpg", "file_size": 1}
        else:
            result = True

        body = json.dumps({"ok": True, "result": result}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _message(self, payload: dict, text: bool = False, photo: bool = False, document: bool = False) -> dict:
        MESSAGE_ID["value"] += 1
        message = {
            "message_id": MESSAGE_ID["value"],
            "date": int(time.time()),
            "chat": {"id": int(payload.get("chat_id") or 1), "type": "private"},
        }
        if text:
            message["text"] = payload.get("text") or ""
        if photo:
            message["photo"] = [
                {"file_id": "PHOTO_FILE_ID", "file_unique_id": "u1", "width": 1280, "height": 720}
            ]
            message["caption"] = payload.get("caption") or ""
        if document:
            message["document"] = {
                "file_id": "DOC_ID",
                "file_unique_id": "u2",
                "file_name": "chat.html",
            }
            message["caption"] = payload.get("caption") or ""
        return message

    do_POST = _handle


def serve(port: int = 8099) -> ThreadingHTTPServer:
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server
