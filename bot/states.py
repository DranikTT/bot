"""FSM-состояния для сценариев с пошаговым вводом."""

from aiogram.fsm.state import State, StatesGroup


class ImportFlow(StatesGroup):
    wait_file = State()


class AdminFlow(StatesGroup):
    wait_user_id = State()
    wait_days = State()
    wait_broadcast = State()
