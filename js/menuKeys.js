'use strict';

const db = require('./database');
const { ADMIN_IDS } = require('./config');

// ID кастомных эмодзи (из исходного проекта — картинки те же).
const EMOJI = Object.freeze({
  back: '5877468380125990242',
  check: '5776375003280838798',
  cross: '5778527486270770928',
  gear: '5877260593903177342',
  plug: '5877465816030515018',
  folder: '5877482652302315100',
  user: '5877477244938489129',
  gem: '5877396173135808022',
  star: '5879785854284599288',
  crown: '5807868868886009920',
  arrow: '5875008416132370818',
  shield: '5778423822940114949',
  clip: '5877495434124988415',
  bell: '5877260593903175642',
  erase: '5814281611541222134'
});

/**
 * Кнопка со стилем и кастомным эмодзи.
 *
 * ВАЖНО: grammy при вызове `.text({ text, callback_data, style })`
 * молча подменяет callback_data на текст кнопки — все кнопки перестают
 * работать («мёртвые» кнопки). Поэтому callback_data передаётся вторым
 * аргументом, а объект идёт первым: .text({...}, data).
 */
function cbButton(kb, { text, data, emoji, style }) {
  const options = { text };

  if (style) options.style = style;
  if (emoji) options.icon_custom_emoji_id = emoji;

  kb.text(options, data);
  return kb;
}

function getUserId(value) {
  if (value && typeof value === 'object') {
    return Number(value.id ?? value.user_id);
  }

  return Number(value);
}

function isAdminUser(userId) {
  const id = getUserId(userId);

  return (
    Array.isArray(ADMIN_IDS) &&
    ADMIN_IDS.some((adminId) => Number(adminId) === id)
  );
}

/**
 * Главное меню — исходный дизайн:
 *   Настройки | Подключить бота
 *   Пригласи друга
 *   Подписка
 *   Админ-панель (только админу)
 */
function getMainMenuKeyboard(userId) {
  const uid = getUserId(userId);
  const keyboard = new InlineKeyboardSafe();

  cbButton(keyboard, {
    text: 'Настройки',
    data: 'menu_settings',
    emoji: EMOJI.gear,
    style: 'primary'
  });
  cbButton(keyboard, {
    text: 'Подключить бота',
    data: 'menu_connect',
    emoji: EMOJI.plug,
    style: 'primary'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Пригласи друга',
    data: 'menu_ref',
    emoji: EMOJI.folder,
    style: 'primary'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Подписка',
    data: 'menu_sub',
    emoji: EMOJI.gem,
    style: 'primary'
  });

  if (isAdminUser(uid)) {
    keyboard.row();
    cbButton(keyboard, {
      text: 'Админ-панель',
      data: 'menu_admin',
      emoji: EMOJI.crown,
      style: 'danger'
    });
  }

  return keyboard;
}

function getSettingsKeyboard(userId) {
  const uid = getUserId(userId);
  const user = db.getUser(uid) || { notify_deleted: 1, notify_edited: 1 };

  const delEnabled = Number(user.notify_deleted) === 1;
  const editEnabled = Number(user.notify_edited) === 1;
  const keyboard = new InlineKeyboardSafe();

  cbButton(keyboard, {
    text: `Удалённые: ${delEnabled ? 'ВКЛ' : 'ВЫКЛ'}`,
    data: 'toggle_deleted',
    emoji: delEnabled ? EMOJI.check : EMOJI.cross,
    style: delEnabled ? 'success' : 'danger'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: `Изменённые: ${editEnabled ? 'ВКЛ' : 'ВЫКЛ'}`,
    data: 'toggle_edited',
    emoji: editEnabled ? EMOJI.check : EMOJI.cross,
    style: editEnabled ? 'success' : 'danger'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Назад',
    data: 'back_main',
    emoji: EMOJI.back,
    style: 'primary'
  });

  return keyboard;
}

function getConnectKeyboard() {
  return cbButton(new InlineKeyboardSafe(), {
    text: 'Проверить подключение',
    data: 'check_connection',
    emoji: EMOJI.check,
    style: 'success'
  })
    .row()
    .text({ text: 'Назад', icon_custom_emoji_id: EMOJI.back, style: 'primary' }, 'back_main');
}

function getSubKeyboard() {
  const keyboard = new InlineKeyboardSafe();

  cbButton(keyboard, {
    text: '7 дней — 25 ⭐️',
    data: 'buy_sub_7',
    emoji: EMOJI.gem,
    style: 'primary'
  });
  cbButton(keyboard, {
    text: '30 дней — 75 ⭐️',
    data: 'buy_sub_30',
    emoji: EMOJI.gem,
    style: 'primary'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: '90 дней — 150 ⭐️',
    data: 'buy_sub_90',
    emoji: EMOJI.gem,
    style: 'primary'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Премиум',
    data: 'menu_premium',
    emoji: EMOJI.star,
    style: 'success'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Назад',
    data: 'back_main',
    emoji: EMOJI.back,
    style: 'primary'
  });

  return keyboard;
}

function getPremiumKeyboard() {
  return cbButton(new InlineKeyboardSafe(), {
    text: 'Купить Премиум — 300 ⭐️',
    data: 'buy_sub_premium',
    emoji: EMOJI.star,
    style: 'success'
  })
    .row()
    .text({ text: 'Назад', icon_custom_emoji_id: EMOJI.back, style: 'primary' }, 'back_main');
}

function getRefKeyboard() {
  return cbButton(new InlineKeyboardSafe(), {
    text: 'Поделиться ссылкой',
    data: 'share_ref',
    emoji: EMOJI.plug,
    style: 'success'
  })
    .row()
    .text({ text: 'Назад', icon_custom_emoji_id: EMOJI.back, style: 'primary' }, 'back_main');
}

function getImportHistoryKeyboard() {
  return cbButton(new InlineKeyboardSafe(), {
    text: 'Назад',
    data: 'back_main',
    emoji: EMOJI.back,
    style: 'primary'
  });
}

function getAdminKeyboard() {
  const keyboard = new InlineKeyboardSafe();

  cbButton(keyboard, {
    text: 'Выдать подписку',
    data: 'admin_give_sub',
    emoji: EMOJI.check,
    style: 'success'
  });
  cbButton(keyboard, {
    text: 'Выдать/Снять Премиум',
    data: 'admin_toggle_prem',
    emoji: EMOJI.star,
    style: 'success'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Забанить юзера',
    data: 'admin_ban_user',
    emoji: EMOJI.cross,
    style: 'danger'
  });
  cbButton(keyboard, {
    text: 'Разбанить юзера',
    data: 'admin_unban_user',
    emoji: EMOJI.check,
    style: 'success'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Забрать подписку',
    data: 'admin_take_sub',
    emoji: EMOJI.cross,
    style: 'danger'
  });
  cbButton(keyboard, {
    text: 'Проверить юзера',
    data: 'admin_check_user',
    emoji: EMOJI.user,
    style: 'primary'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Рассылка всем',
    data: 'admin_broadcast',
    emoji: EMOJI.plug,
    style: 'primary'
  });
  keyboard.row();

  cbButton(keyboard, {
    text: 'Назад в меню',
    data: 'back_main',
    emoji: EMOJI.back,
    style: 'primary'
  });

  return keyboard;
}

module.exports = {
  EMOJI,
  cbButton,
  InlineKeyboardSafe,
  getMainMenuKeyboard,
  getSettingsKeyboard,
  getConnectKeyboard,
  getSubKeyboard,
  getPremiumKeyboard,
  getRefKeyboard,
  getImportHistoryKeyboard,
  getAdminKeyboard
};

// grammy подключается лениво: модуль импортируется и тестами, которым
// Telegram-клиент не нужен.
function InlineKeyboardSafe() {
  const { InlineKeyboard } = require('grammy');
  return new InlineKeyboard();
}
