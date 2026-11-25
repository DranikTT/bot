"""Inline-клавиатуры.

Дизайн — как в присланном примере: короткие подписи, по две кнопки в ряд,
акцентные кнопки отдельными рядами, callback_data лаконичные (латиницей).
"""

from __future__ import annotations

from aiogram.types import InlineKeyboardButton, InlineKeyboardMarkup

from . import database as db
from .config import SUB_PRICES, is_admin

# Кастомные эмодзи (премиум-эмодзи Telegram). aiogram >= 3.28 умеет их
# отправлять в кнопках; на старых версиях поле просто игнорируется клиентом.
EMOJI = {
    "back": "5877468380125990242",
    "check": "5776375003280838798",
    "cross": "5778527486270770928",
    "gear": "5877260593903177342",
    "plug": "5877465816030515018",
    "folder": "5877482652302315100",
    "user": "5877477244938489129",
    "gem": "5877396173135808022",
    "star": "5879785854284599288",
    "crown": "5807868868886009920",
    "shield": "5778423822940114949",
    "clip": "5877495434124988415",
}


def btn(
    text: str,
    callback_data: str,
    *,
    icon: str | None = None,
    style: str | None = None,
) -> InlineKeyboardButton:
    """Кнопка с callback_data.

    В aiogram конструктор тоже принимает callback_data отдельным полем, но
    держим единую обёртку: так невозможно случайно положить данные в текст
    (именно этот баг ломал все кнопки в прежней версии).
    """
    if not callback_data or len(callback_data.encode()) > 64:
        raise ValueError(f"Некорректный callback_data: {callback_data!r}")

    kwargs: dict[str, str] = {"text": text, "callback_data": callback_data}
    if icon:
        kwargs["icon_custom_emoji_id"] = icon
    if style:
        kwargs["style"] = style

    try:
        return InlineKeyboardButton(**kwargs)
    except TypeError:
        # aiogram < 3.28 не знает icon_custom_emoji_id/style — отдаём базовую кнопку
        kwargs.pop("icon_custom_emoji_id", None)
        kwargs.pop("style", None)
        return InlineKeyboardButton(**kwargs)


def kb(*rows: list[InlineKeyboardButton]) -> InlineKeyboardMarkup:
    return InlineKeyboardMarkup(inline_keyboard=[row for row in rows if row])


def back_row(label: str = "Назад") -> list[InlineKeyboardButton]:
    return [btn(label, "back_main", icon=EMOJI["back"], style="primary")]


# --------------------------------------------------------------------------
# Меню
# --------------------------------------------------------------------------


def main_menu_keyboard(user_id: int) -> InlineKeyboardMarkup:
    rows = [
        [
            btn("Тарифы", "tariffs", icon=EMOJI["gem"], style="primary"),
            btn("Пригласить друга", "invite", icon=EMOJI["folder"], style="primary"),
        ],
        [
            btn("Подключение", "connect", icon=EMOJI["plug"], style="primary"),
            btn("Создать бота", "create_bot", icon=EMOJI["star"], style="success"),
        ],
        [btn("Настройки", "settings", icon=EMOJI["gear"], style="primary")],
    ]

    if is_admin(user_id):
        rows.append([btn("Админ-панель", "admin", icon=EMOJI["crown"], style="danger")])

    return kb(*rows)


def settings_keyboard(user_id: int) -> InlineKeyboardMarkup:
    user = db.get_user(user_id) or {}
    deleted_on = int(user.get("notify_deleted", 1) or 0) == 1
    edited_on = int(user.get("notify_edited", 1) or 0) == 1

    return kb(
        [
            btn(
                f"Удалённые: {'ВКЛ' if deleted_on else 'ВЫКЛ'}",
                "toggle_deleted",
                icon=EMOJI["check"] if deleted_on else EMOJI["cross"],
                style="success" if deleted_on else "danger",
            )
        ],
        [
            btn(
                f"Изменённые: {'ВКЛ' if edited_on else 'ВЫКЛ'}",
                "toggle_edited",
                icon=EMOJI["check"] if edited_on else EMOJI["cross"],
                style="success" if edited_on else "danger",
            )
        ],
        back_row(),
    )


def connect_keyboard() -> InlineKeyboardMarkup:
    return kb(
        [btn("Проверить подключение", "check_connection", icon=EMOJI["check"], style="success")],
        [btn("Как подключить", "connect_help", icon=EMOJI["clip"], style="primary")],
        back_row(),
    )


def tariffs_keyboard() -> InlineKeyboardMarkup:
    return kb(
        [
            btn(f"7 дней — {SUB_PRICES['days7']} ⭐", "buy_sub_7", icon=EMOJI["gem"], style="primary"),
            btn(f"30 дней — {SUB_PRICES['days30']} ⭐", "buy_sub_30", icon=EMOJI["gem"], style="primary"),
        ],
        [btn(f"90 дней — {SUB_PRICES['days90']} ⭐", "buy_sub_90", icon=EMOJI["gem"], style="primary")],
        [btn("Премиум", "premium", icon=EMOJI["star"], style="success")],
        back_row(),
    )


def premium_keyboard() -> InlineKeyboardMarkup:
    return kb(
        [btn(f"Купить Премиум — {SUB_PRICES['premium']} ⭐", "buy_sub_premium", icon=EMOJI["star"], style="success")],
        [btn("Все тарифы", "tariffs", icon=EMOJI["gem"], style="primary")],
        back_row(),
    )


def invite_keyboard(username: str) -> InlineKeyboardMarkup:
    return kb(
        [btn("Поделиться ссылкой", "share_link", icon=EMOJI["folder"], style="success")],
        [btn("Открыть в Telegram", "invite_url", icon=EMOJI["plug"], style="primary")],
        back_row(),
    )


def create_bot_keyboard(username: str) -> InlineKeyboardMarkup:
    return kb(
        [btn("Открыть BotFather", "open_botfather", icon=EMOJI["star"], style="success")],
        [btn("Проверить подключение", "create_bot_check", icon=EMOJI["check"], style="primary")],
        back_row(),
    )


def archive_types_keyboard(selected: set[str], labels: dict[str, str]) -> InlineKeyboardMarkup:
    rows: list[list[InlineKeyboardButton]] = []
    keys = list(labels)

    for index in range(0, len(keys), 2):
        row = []
        for key in keys[index:index + 2]:
            active = key in selected
            row.append(
                btn(
                    f"{labels[key]}",
                    f"arch_toggle_{key}",
                    icon=EMOJI["check"] if active else EMOJI["cross"],
                    style="success" if active else "primary",
                )
            )
        rows.append(row)

    rows.append(
        [
            btn("Отменить", "arch_cancel", icon=EMOJI["cross"], style="danger"),
            btn("Начать экспорт", "arch_start", icon=EMOJI["check"], style="success"),
        ]
    )
    return kb(*rows)


def admin_keyboard() -> InlineKeyboardMarkup:
    return kb(
        [
            btn("Выдать подписку", "admin_give_sub", icon=EMOJI["check"], style="success"),
            btn("Забрать подписку", "admin_take_sub", icon=EMOJI["cross"], style="danger"),
        ],
        [
            btn("Выдать/снять Премиум", "admin_toggle_prem", icon=EMOJI["star"], style="success"),
            btn("Проверить юзера", "admin_check_user", icon=EMOJI["user"], style="primary"),
        ],
        [
            btn("Забанить", "admin_ban_user", icon=EMOJI["cross"], style="danger"),
            btn("Разбанить", "admin_unban_user", icon=EMOJI["check"], style="success"),
        ],
        [btn("Рассылка всем", "admin_broadcast", icon=EMOJI["plug"], style="primary")],
        back_row("Назад в меню"),
    )


def import_result_keyboard(ok: bool) -> InlineKeyboardMarkup:
    if ok:
        return kb(back_row("Назад в меню"))
    return kb(
        [btn("Повторить импорт", "archive_import_again", style="primary")],
        back_row("Назад в меню"),
    )


def import_wait_keyboard() -> InlineKeyboardMarkup:
    return kb([btn("Отмена", "archive_import_cancel", style="danger")])


def admin_back_keyboard() -> InlineKeyboardMarkup:
    return kb(back_row("Назад в админ-панель"))


def chats_keyboard(chats: list[dict]) -> InlineKeyboardMarkup:
    rows = [
        [btn(str(chat.get("title") or "Чат")[:48], f"arch_sel_{chat['chat_id']}", icon=EMOJI["user"], style="primary")]
        for chat in chats[:35]
    ]
    rows.append(back_row())
    return kb(*rows)


def empty_chats_keyboard() -> InlineKeyboardMarkup:
    return kb(
        [btn("Подключить бота", "connect", icon=EMOJI["plug"], style="primary")],
        back_row(),
    )
