'use strict';

const { InlineKeyboard } = require('grammy');
const db = require('./database');
const { ADMIN_IDS } = require('./config');
const { callbackButton } = require('./keyboardGuard');

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
    .add(callbackButton('Настройки', 'menu_settings', { icon_custom_emoji_id: EMOJI.gear, style: 'primary' }))
    .add(callbackButton('Подключить бота', 'menu_connect', { icon_custom_emoji_id: EMOJI.plug, style: 'primary' }))
    .row()
    .add(callbackButton('Пригласи друга', 'menu_ref', { icon_custom_emoji_id: EMOJI.folder, style: 'primary' }))
    .row()
    .add(callbackButton('Подписка', 'menu_sub', { icon_custom_emoji_id: EMOJI.gem, style: 'primary' }));

  if (isAdminUser(uid)) {
    keyboard
      .row()
      .add(callbackButton('Админ-панель', 'menu_admin', { icon_custom_emoji_id: EMOJI.crown, style: 'danger' }));
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
    .add(
      callbackButton(
        `Удалённые: ${delEnabled ? 'ВКЛ' : 'ВЫКЛ'}`,
        'toggle_deleted',
        {
          icon_custom_emoji_id: delEnabled ? EMOJI.check : EMOJI.cross,
          style: delEnabled ? 'success' : 'danger'
        }
      )
    )
    .row()
    .add(
      callbackButton(
        `Изменённые: ${editEnabled ? 'ВКЛ' : 'ВЫКЛ'}`,
        'toggle_edited',
        {
          icon_custom_emoji_id: editEnabled ? EMOJI.check : EMOJI.cross,
          style: editEnabled ? 'success' : 'danger'
        }
      )
    )
    .row()
    .add(callbackButton('Назад', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));

  return keyboard;
}

function getConnectKeyboard() {
  return new InlineKeyboard()
    .add(callbackButton('Проверить подключение', 'check_connection', { icon_custom_emoji_id: EMOJI.check, style: 'success' }))
    .row()
    .add(callbackButton('Назад', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));
}

function getSubKeyboard() {
  return new InlineKeyboard()
    .add(callbackButton('7 дней — 25 ⭐️', 'buy_sub_7', { icon_custom_emoji_id: EMOJI.gem, style: 'primary' }))
    .add(callbackButton('30 дней — 75 ⭐️', 'buy_sub_30', { icon_custom_emoji_id: EMOJI.gem, style: 'primary' }))
    .row()
    .add(callbackButton('90 дней — 150 ⭐️', 'buy_sub_90', { icon_custom_emoji_id: EMOJI.gem, style: 'primary' }))
    .row()
    .add(callbackButton('Премиум', 'menu_premium', { icon_custom_emoji_id: EMOJI.star, style: 'success' }))
    .row()
    .add(callbackButton('Назад', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));
}

function getPremiumKeyboard() {
  return new InlineKeyboard()
    .add(callbackButton('Купить Премиум — 300 ⭐️', 'buy_sub_premium', { icon_custom_emoji_id: EMOJI.star, style: 'success' }))
    .row()
    .add(callbackButton('Назад', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));
}

function getRefKeyboard() {
  // switch_inline_query и оформление собираем вручную: у switchInline()
  // вторым аргументом идёт query, а не объект, — иначе в кнопку попадают
  // лишние поля.
  return new InlineKeyboard()
    .add({ text: 'Поделиться ссылкой', switch_inline_query: '', icon_custom_emoji_id: EMOJI.plug, style: 'success' })
    .row()
    .add(callbackButton('Назад', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));
}

function getImportHistoryKeyboard() {
  return new InlineKeyboard()
    .add(callbackButton('Назад', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));
}

function getAdminKeyboard() {
  return new InlineKeyboard()
    .add(callbackButton('Выдать подписку', 'admin_give_sub', { icon_custom_emoji_id: EMOJI.check, style: 'success' }))
    .add(callbackButton('Выдать/Снять Премиум', 'admin_toggle_prem', { icon_custom_emoji_id: EMOJI.star, style: 'success' }))
    .row()
    .add(callbackButton('Забанить юзера', 'admin_ban_user', { icon_custom_emoji_id: EMOJI.cross, style: 'danger' }))
    .add(callbackButton('Разбанить юзера', 'admin_unban_user', { icon_custom_emoji_id: EMOJI.check, style: 'success' }))
    .row()
    .add(callbackButton('Забрать подписку', 'admin_take_sub', { icon_custom_emoji_id: EMOJI.cross, style: 'danger' }))
    .add(callbackButton('Проверить юзера', 'admin_check_user', { icon_custom_emoji_id: EMOJI.user, style: 'primary' }))
    .row()
    .add(callbackButton('Рассылка всем', 'admin_broadcast', { icon_custom_emoji_id: EMOJI.plug, style: 'primary' }))
    .row()
    .add(callbackButton('Назад в меню', 'back_main', { icon_custom_emoji_id: EMOJI.back, style: 'primary' }));
}

module.exports = {
  EMOJI,
  callbackButton,
  getMainMenuKeyboard,
  getSettingsKeyboard,
  getConnectKeyboard,
  getSubKeyboard,
  getPremiumKeyboard,
  getRefKeyboard,
  getImportHistoryKeyboard,
  getAdminKeyboard
};
