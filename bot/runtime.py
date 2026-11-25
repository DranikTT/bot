"""Общие изменяемые состояния процесса (сессии, флаги, счётчики)."""

from __future__ import annotations

from typing import Any

# Сессии экспорта: user_id -> {chat_id, title, types}
archive_sessions: dict[int, dict[str, Any]] = {}

# Ожидание ввода в админ-панели: user_id -> действие
admin_states: dict[int, str] = {}

# Временные наборы id сообщений, которые бот сгенерировал сам
bot_generated_ids: set[int] = set()
muted_deleted_ids: set[int] = set()
