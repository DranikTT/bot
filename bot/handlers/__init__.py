"""Роутеры и общий регистратор обработчиков."""

from __future__ import annotations

from aiogram import Dispatcher

from . import admin, messages, payments, user


def register_handlers(dp: Dispatcher) -> None:
    """Порядок важен: админские фильтры раньше пользовательских."""
    dp.include_router(admin.router)
    dp.include_router(payments.router)
    dp.include_router(user.router)
    dp.include_router(messages.router)


__all__ = ["register_handlers"]
