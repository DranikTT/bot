'use strict';

const { InlineKeyboard } = require('grammy');
const db = require('./database');
const { ADMIN_IDS } = require('./config');

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

function getMainMenuKeyboard(userId) {
  const uid = getUserId(userId);
  const keyboard = new InlineKeyboard();

  keyboard
    .text({ text: 'Настройки', callback_data: 'menu_settings', icon_custom_emoji_id: EMOJI.gear, style: 'primary' })
    .text({ text: 'Подключить бота', callback_data: 'menu_connect', icon_custom_emoji_id: EMOJI.plug, style: 'primary' })
    .row()
    .text({ text: 'Пригласи друга', callback_data: 'menu_ref', icon_custom_emoji_id: EMOJI.folder, style: 'primary' })
    .row()
    .text({ text: 'Подписка', callback_data: 'menu_sub', icon_custom_emoji_id: EMOJI.gem, style: 'primary' });

  if (isAdminUser(uid)) {
    keyboard
      .row()
      .text({ text: 'Админ-панель', callback_data: 'menu_admin', icon_custom_emoji_id: EMOJI.crown, style: 'danger' });
  }

  return keyboard;
}

function getSettingsKeyboard(userId) {
  const uid = getUserId(userId);
  const user = db.getUser(uid) || { notify_deleted: 1, notify_edited: 1 };

  const delEnabled = Number(user.notify_deleted) === 1;
  const editEnabled = Number(user.notify_edited) === 1;
  const keyboard = new InlineKeyboard();

  keyboard
    .text({
      text: `Удалённые: ${delEnabled ? 'ВКЛ' : 'ВЫКЛ'}`,
      callback_data: 'toggle_deleted',
      icon_custom_emoji_id: delEnabled ? EMOJI.check : EMOJI.cross,
      style: delEnabled ? 'success' : 'danger'
    })
    .row()
    .text({
      text: `Изменённые: ${editEnabled ? 'ВКЛ' : 'ВЫКЛ'}`,
      callback_data: 'toggle_edited',
      icon_custom_emoji_id: editEnabled ? EMOJI.check : EMOJI.cross,
      style: editEnabled ? 'success' : 'danger'
    })
    .row()
    .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });

  return keyboard;
}

function getConnectKeyboard() {
  return new InlineKeyboard()
    .text({ text: 'Проверить подключение', callback_data: 'check_connection', icon_custom_emoji_id: EMOJI.check, style: 'success' })
    .row()
    .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });
}

function getSubKeyboard() {
  return new InlineKeyboard()
    .text({ text: '7 дней — 25 ⭐️', callback_data: 'buy_sub_7', icon_custom_emoji_id: EMOJI.gem, style: 'primary' })
    .text({ text: '30 дней — 75 ⭐️', callback_data: 'buy_sub_30', icon_custom_emoji_id: EMOJI.gem, style: 'primary' })
    .row()
    .text({ text: '90 дней — 150 ⭐️', callback_data: 'buy_sub_90', icon_custom_emoji_id: EMOJI.gem, style: 'primary' })
    .row()
    .text({ text: 'Премиум', callback_data: 'menu_premium', icon_custom_emoji_id: EMOJI.star, style: 'success' })
    .row()
    .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });
}

function getPremiumKeyboard() {
  return new InlineKeyboard()
    .text({ text: 'Купить Премиум — 300 ⭐️', callback_data: 'buy_sub_premium', icon_custom_emoji_id: EMOJI.star, style: 'success' })
    .row()
    .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });
}

function getRefKeyboard() {
  return new InlineKeyboard()
    .switchInline({ text: 'Поделиться ссылкой', query: '', icon_custom_emoji_id: EMOJI.plug, style: 'success' })
    .row()
    .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });
}

function getImportHistoryKeyboard() {
  return new InlineKeyboard()
    .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });
}

function getAdminKeyboard() {
  return new InlineKeyboard()
    .text({ text: 'Выдать подписку', callback_data: 'admin_give_sub', icon_custom_emoji_id: EMOJI.check, style: 'success' })
    .text({ text: 'Выдать/Снять Премиум', callback_data: 'admin_toggle_prem', icon_custom_emoji_id: EMOJI.star, style: 'success' })
    .row()
    .text({ text: 'Забанить юзера', callback_data: 'admin_ban_user', icon_custom_emoji_id: EMOJI.cross, style: 'danger' })
    .text({ text: 'Разбанить юзера', callback_data: 'admin_unban_user', icon_custom_emoji_id: EMOJI.check, style: 'success' })
    .row()
    .text({ text: 'Забрать подписку', callback_data: 'admin_take_sub', icon_custom_emoji_id: EMOJI.cross, style: 'danger' })
    .text({ text: 'Проверить юзера', callback_data: 'admin_check_user', icon_custom_emoji_id: EMOJI.user, style: 'primary' })
    .row()
    .text({ text: 'Рассылка всем', callback_data: 'admin_broadcast', icon_custom_emoji_id: EMOJI.plug, style: 'primary' })
    .row()
    .text({ text: 'Назад в меню', callback_data: 'back_main', icon_custom_emoji_id: EMOJI.back, style: 'primary' });
}

module.exports = {
  EMOJI,
  getMainMenuKeyboard,
  getSettingsKeyboard,
  getConnectKeyboard,
  getSubKeyboard,
  getPremiumKeyboard,
  getRefKeyboard,
  getImportHistoryKeyboard,
  getAdminKeyboard
};
