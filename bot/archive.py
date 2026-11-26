"""Экспорт переписки в HTML и импорт экспорта Telegram Desktop."""

from __future__ import annotations

import base64
import io
import json
import zipfile
from datetime import datetime
from pathlib import Path
from typing import Any

from . import database as db
from .menu import ARCHIVE_TYPE_LABELS, escape_html

# --------------------------------------------------------------------------
# Нормализация типов
# --------------------------------------------------------------------------

TYPE_ALIASES = {
    "voice_message": "voice",
    "round_video": "video_note",
    "video_message": "video_note",
    "video_file": "video",
    "audio_file": "audio",
    "gif": "animation",
}


def normalize_type(value: str | None) -> str:
    kind = str(value or "text").lower()
    return TYPE_ALIASES.get(kind, kind)


def _as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    return str(value).lower() in {"1", "true", "yes"}


def _file_to_base64(path: Path | None, max_bytes: int) -> tuple[str, str] | None:
    """Читает локальный файл в base64, если он не больше лимита."""
    if not path or not path.is_file():
        return None

    try:
        if path.stat().st_size > max_bytes:
            return None
        data = base64.b64encode(path.read_bytes()).decode()
    except OSError:
        return None

    suffix = path.suffix.lower()
    mime = {
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
        ".png": "image/png",
        ".webp": "image/webp",
        ".mp4": "video/mp4",
        ".webm": "video/webm",
        ".ogg": "audio/ogg",
        ".oga": "audio/ogg",
        ".mp3": "audio/mpeg",
        ".m4a": "audio/mp4",
    }.get(suffix, "application/octet-stream")

    return data, mime


# --------------------------------------------------------------------------
# Построение HTML
# --------------------------------------------------------------------------

_CSS = """
:root{--bg:#0e1621;--chat:#17212b;--in:#182533;--out:#2b5278;--text:#f5f5f5;
--sub:#7f91a4;--accent:#5288c1;--border:#242f3d;--radius:14px}
*{box-sizing:border-box;margin:0;padding:0;-webkit-tap-highlight-color:transparent}
body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,
sans-serif;background:var(--bg);color:var(--text);display:flex;justify-content:center;
min-height:100vh;padding:12px}
.archive{width:100%;max-width:860px;background:var(--chat);border-radius:12px;
overflow:hidden;display:flex;flex-direction:column;box-shadow:0 8px 30px rgba(0,0,0,.5)}
.head{padding:16px 20px;background:var(--chat);border-bottom:1px solid var(--border);
display:flex;justify-content:space-between;align-items:center;position:sticky;top:0;z-index:100}
.title{font-size:1.15rem;font-weight:600;color:#fff}
.info{font-size:.85rem;color:var(--sub);margin-top:2px}
.stats{text-align:right;font-size:.85rem;color:var(--accent);font-weight:500}
.list{padding:20px 16px;display:flex;flex-direction:column;gap:12px;flex:1}
.row{display:flex;width:100%}
.row.in{justify-content:flex-start}
.row.out{justify-content:flex-end}
.bubble{max-width:78%;padding:10px 14px;border-radius:var(--radius);position:relative;
word-wrap:break-word;font-size:.95rem;line-height:1.45;box-shadow:0 1px 2px rgba(0,0,0,.2)}
.bubble.sticker-only{background:transparent!important;box-shadow:none!important;padding:2px!important}
.row.in .bubble:not(.sticker-only){background:var(--in);border-bottom-left-radius:4px}
.row.out .bubble:not(.sticker-only){background:var(--out);border-bottom-right-radius:4px}
.bhead{display:flex;justify-content:space-between;align-items:baseline;gap:14px;
font-size:.75rem;color:var(--sub);margin-bottom:4px}
.author{font-weight:600;color:var(--accent)}
.row.out .author{color:#92c5ff}
.btxt{margin-top:2px;word-break:break-word}
.media{margin-top:8px;border-radius:8px;overflow:hidden}
.media.photo img{max-width:100%;max-height:480px;border-radius:8px;display:block;object-fit:cover}
.media.video video{max-width:100%;max-height:480px;border-radius:8px;display:block}
.media.sticker{background:transparent!important;padding:0;width:170px;height:170px}
.media.sticker img,.media.sticker video{width:100%;height:100%;object-fit:contain;display:block}
.media.round{display:flex;justify-content:center;align-items:center;padding:4px}
.media.round video{width:230px;height:230px;border-radius:50%;object-fit:cover;
display:block;border:2px solid rgba(255,255,255,.1)}
.media.voice audio{width:100%;min-width:250px;height:40px;margin-top:6px;outline:none}
.file{display:flex;align-items:center;gap:10px;padding:8px 10px;
background:rgba(0,0,0,.15);border-radius:8px;margin-top:6px}
.ficon{font-size:1.6rem}
.fname{font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.fsub{font-size:.75rem;color:var(--sub)}
.placeholder{margin-top:6px;font-style:italic;color:var(--sub);font-size:.85rem}
@media(max-width:650px){body{padding:0}.archive{border-radius:0;min-height:100vh}
.bubble{max-width:88%}.media.round video{width:190px;height:190px}
.media.voice audio{min-width:210px}}
"""


def _format_date(timestamp: int | float | None) -> str:
    if not timestamp:
        return ""
    try:
        return datetime.fromtimestamp(int(timestamp)).strftime("%d.%m.%Y %H:%M:%S")
    except (OverflowError, OSError, ValueError):
        return ""


def _media_html(message: dict[str, Any], max_media_bytes: int) -> tuple[str, bool]:
    """Возвращает (html, только_стикер)."""
    kind = normalize_type(message.get("type") or message.get("media_type"))
    media_path = message.get("media_path")
    path = Path(media_path) if media_path else None
    encoded = _file_to_base64(path, max_media_bytes)

    if kind == "photo":
        if encoded:
            data, mime = encoded
            return f'<div class="media photo"><img src="data:{mime};base64,{data}" alt="Фото" loading="lazy"></div>', False
        return '<div class="placeholder">[📷 Фотография]</div>', False

    if kind == "sticker":
        if encoded:
            data, mime = encoded
            if "webm" in mime:
                return f'<div class="media sticker"><video src="data:{mime};base64,{data}" autoplay loop muted playsinline></video></div>', True
            return f'<div class="media sticker"><img src="data:{mime};base64,{data}" alt="Стикер" loading="lazy"></div>', True
        return '<div class="placeholder">[🎭 Стикер]</div>', True

    if kind == "voice":
        if encoded:
            data, mime = encoded
            return (
                f'<div class="media voice"><audio controls preload="none">'
                f'<source src="data:{mime};base64,{data}" type="{mime}"></audio></div>',
                False,
            )
        return '<div class="placeholder">[🎤 Голосовое]</div>', False

    if kind == "video_note":
        if encoded:
            data, mime = encoded
            return f'<div class="media round"><video src="data:{mime};base64,{data}" controls playsinline preload="metadata"></video></div>', False
        return '<div class="placeholder">[🔘 Кружок]</div>', False

    if kind == "video":
        if encoded:
            data, mime = encoded
            return f'<div class="media video"><video src="data:{mime};base64,{data}" controls playsinline preload="metadata"></video></div>', False
        return '<div class="placeholder">[🎬 Видео]</div>', False

    if kind == "animation":
        if encoded:
            data, mime = encoded
            return f'<div class="media photo"><video src="data:{mime};base64,{data}" autoplay loop muted playsinline></video></div>', False
        return '<div class="placeholder">[🎞 Гиф]</div>', False

    if kind == "document":
        name = escape_html(message.get("file_name") or "Документ")
        return (
            f'<div class="file"><div class="ficon">📁</div>'
            f'<div><div class="fname">{name}</div>'
            f'<div class="fsub">Вложенный файл</div></div></div>',
            False,
        )

    if kind == "location":
        return '<div class="placeholder">[📍 Геолокация]</div>', False

    return "", False


def build_chat_html(
    chat_title: str,
    chat_id: str | int,
    messages: list[dict[str, Any]],
    *,
    max_media_bytes: int = 5 * 1024 * 1024,
) -> str:
    items: list[str] = []

    for index, message in enumerate(messages, start=1):
        outgoing = _as_bool(message.get("is_outgoing"))
        author = "Вы" if outgoing else escape_html(chat_title or f"Чат {chat_id}")
        media_html, sticker_only = _media_html(message, max_media_bytes)

        text = message.get("text") or ""
        text_html = (
            f'<div class="btxt">{escape_html(text).replace(chr(10), "<br>")}</div>'
            if str(text).strip()
            else ""
        )

        items.append(
            f'<div class="row {"out" if outgoing else "in"}" id="msg-{message.get("id", index)}">'
            f'<div class="bubble {"sticker-only" if sticker_only else ""}">'
            f'<div class="bhead"><span class="author">{author}</span>'
            f'<span>{_format_date(message.get("date"))}</span></div>'
            f"{text_html}{media_html}</div></div>"
        )

    return (
        '<!DOCTYPE html><html lang="ru"><head><meta charset="UTF-8">'
        '<meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no">'
        f"<title>Архив: {escape_html(chat_title)}</title><style>{_CSS}</style></head><body>"
        '<div class="archive"><div class="head"><div>'
        f'<div class="title">{escape_html(chat_title)}</div>'
        f'<div class="info">Идентификатор чата: {escape_html(chat_id)}</div></div>'
        f'<div class="stats"><div>Всего сообщений: {len(messages)}</div>'
        '<div style="font-size:.75rem;color:var(--sub);margin-top:2px">Экспорт завершён</div>'
        f'</div></div><div class="list">{"".join(items)}</div></div></body></html>'
    )


# --------------------------------------------------------------------------
# Подготовка сообщений из БД
# --------------------------------------------------------------------------


def normalize_db_message(row: dict[str, Any], owner_id: int, index: int) -> dict[str, Any]:
    from_id = row.get("from_id") or row.get("sender_id")
    row_owner = row.get("owner_id") or owner_id

    if row.get("is_outgoing") is not None:
        outgoing = _as_bool(row["is_outgoing"])
    else:
        outgoing = from_id is not None and str(from_id).replace("user", "") == str(row_owner).replace("user", "")

    return {
        **row,
        "id": row.get("id") or row.get("message_id") or index + 1,
        "date": row.get("date") or row.get("created_at") or 0,
        "is_outgoing": outgoing,
        "text": row.get("text") or row.get("caption") or "",
        "type": normalize_type(row.get("type") or row.get("media_type")),
        "media_path": row.get("media_path") or row.get("local_path") or row.get("file_path"),
    }


def generate_html_archive(
    owner_id: int,
    chat_id: str,
    chat_title: str,
    allowed_types: set[str],
    *,
    max_media_bytes: int = 5 * 1024 * 1024,
) -> tuple[str, int]:
    """Возвращает (html, количество сообщений)."""
    rows = db.get_chat_messages(chat_id, owner_id)
    allowed = {normalize_type(kind) for kind in (allowed_types or ARCHIVE_TYPE_LABELS)}

    messages: list[dict[str, Any]] = []
    for index, row in enumerate(rows):
        message = normalize_db_message(row, owner_id, index)
        if normalize_type(message["type"]) not in allowed:
            continue
        messages.append(message)

    title = chat_title or f"Чат {chat_id}"
    return build_chat_html(title, chat_id, messages, max_media_bytes=max_media_bytes), len(messages)


# --------------------------------------------------------------------------
# Импорт Telegram Desktop
# --------------------------------------------------------------------------


def _parse_desktop_text(value: Any) -> str:
    if not value:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        parts = []
        for item in value:
            if isinstance(item, str):
                parts.append(item)
            elif isinstance(item, dict):
                parts.append(str(item.get("text") or ""))
        return "".join(parts)
    return str(value)


def import_telegram_desktop(json_path: Path, owner_id: int) -> int:
    """Импортирует result.json в БД. Возвращает число сообщений."""
    data = json.loads(Path(json_path).read_text(encoding="utf-8"))

    messages = data.get("messages")
    if not isinstance(messages, list):
        raise ValueError("Некорректная структура result.json: нет массива messages.")

    target_chat_id = str(data.get("id") or f"imported_{int(datetime.now().timestamp())}")
    chat_title = str(data.get("name") or f"Чат {target_chat_id}")
    count = 0

    for item in messages:
        if item.get("type") != "message":
            continue

        sender_id = str(item.get("from_id") or "0").replace("user", "")

        media_type = "text"
        if item.get("photo"):
            media_type = "photo"
        elif item.get("media_type"):
            media_type = normalize_type(item["media_type"])
        elif item.get("file"):
            media_type = "document"

        timestamp = item.get("date_unixtime")
        try:
            date_value = int(timestamp) if timestamp else int(
                datetime.fromisoformat(str(item.get("date")).replace("Z", "+00:00")).timestamp()
            )
        except (TypeError, ValueError):
            date_value = None

        db.save_message(
            chat_id=target_chat_id,
            message_id=int(item.get("id") or count + 1),
            owner_id=owner_id,
            sender_id=int(sender_id) if sender_id.isdigit() else None,
            sender_name=str(item.get("from") or ""),
            peer_name=chat_title,
            text=_parse_desktop_text(item.get("text")),
            media_type=media_type,
            file_name=item.get("file") if isinstance(item.get("file"), str) else None,
            date=date_value,
            is_outgoing=sender_id == str(owner_id),
        )
        count += 1

    return count


def extract_zip(zip_path: Path, destination: Path) -> Path:
    """Безопасно распаковывает ZIP и возвращает путь к result.json."""
    destination.mkdir(parents=True, exist_ok=True)
    root = destination.resolve()

    with zipfile.ZipFile(zip_path) as archive:
        for member in archive.infolist():
            # Защита от zip-slip: пути вне каталога распаковки игнорируем
            target = (root / member.filename).resolve()
            if not str(target).startswith(str(root)):
                continue
            if member.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(member) as source, open(target, "wb") as output:
                output.write(source.read())

    for candidate in root.rglob("result.json"):
        return candidate

    raise ValueError("В ZIP-архиве не найден файл result.json.")


def read_text_preview(path: Path, limit_bytes: int = 64 * 1024) -> io.BytesIO:
    return io.BytesIO(path.read_bytes()[:limit_bytes])
