'use strict';

const fs = require('fs');
const path = require('path');
const { InputFile, InlineKeyboard } = require('grammy');

const db = require('./database');
const { ADMIN_IDS, botInfo } = require('./config');
const keys = require('./menuKeys');

const MENU_BANNERS = Object.freeze({
  main: path.join(__dirname, 'image', 'mainmenu.jpg'),
  admin: path.join(__dirname, 'image', 'adminmenu.jpg'),
  subscription: path.join(__dirname, 'image', 'subscription.jpg'),
  settings: path.join(__dirname, 'image', 'settings.jpg')
});

const ARCHIVE_TYPE_LABELS = Object.freeze({
  text: '💬 Сообщение',
  photo: '🖼 Фото',
  video: '🎬 Видео',
  audio: '🎵 Аудио',
  document: '📎 Документ',
  animation: '🎞 Гиф',
  video_note: '🔘 Кружок',
  voice: '🎤 Голосовое сообщение',
  location: '📍 Геолокация',
  sticker: '✨ Стикер'
});

const bannerFileIdCache = {};

const isAdminUser = (userId) =>
  Array.isArray(ADMIN_IDS) &&
  ADMIN_IDS.some((adminId) => Number(adminId) === Number(userId));

function getUserId(value) {
  if (value && typeof value === 'object') {
    return Number(value.id || value.user_id);
  }

  return Number(value);
}

function escapeHtml(text) {
  if (text === null || text === undefined) return '';

  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function formatUserNick(userObj) {
  if (!userObj) return 'Пользователь';

  const name =
    [userObj.first_name, userObj.last_name].filter(Boolean).join(' ') ||
    'Пользователь';

  return userObj.username
    ? `${escapeHtml(name)} (@${escapeHtml(userObj.username)})`
    : escapeHtml(name);
}

function formatDateTime(unixSeconds) {
  const seconds = Number(unixSeconds);
  if (!Number.isFinite(seconds) || seconds <= 0) return 'нет';

  const date = new Date(seconds * 1000);
  const pad = (number) => String(number).padStart(2, '0');

  return `${pad(date.getDate())}.${pad(date.getMonth() + 1)}.${date.getFullYear()} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function formatShortDate(unixSeconds) {
  const seconds = Number(unixSeconds);
  const timestamp =
    Number.isFinite(seconds) && seconds > 0
      ? seconds
      : Math.floor(Date.now() / 1000);

  const date = new Date(timestamp * 1000);
  const pad = (number) => String(number).padStart(2, '0');

  return `${pad(date.getDate())}.${pad(date.getMonth() + 1)}.${date.getFullYear()} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function formatSubStatus(user) {
  if (!user) return 'не активна';
  if (Number(user.is_banned) === 1) return 'заблокирована (Бан)';
  if (isAdminUser(user.user_id || user.id)) return 'Безлимит (Админ)';

  const now = Math.floor(Date.now() / 1000);
  const subUntil = Number(user.subscription_until || user.sub_until || 0);

  if (subUntil > now) {
    const daysLeft = Math.ceil((subUntil - now) / 86400);
    const premium = Number(user.is_premium) === 1 ? 'Премиум ' : '';
    return `${premium}активна до ${formatDateTime(subUntil)} (${daysLeft} дн.)`;
  }

  return 'не активна';
}

function buildMenu(text, keyboard, bannerKey) {
  return {
    text,
    reply_markup: keyboard,
    parse_mode: 'HTML',
    ...(bannerKey && MENU_BANNERS[bannerKey]
      ? { banner: MENU_BANNERS[bannerKey], bannerKey }
      : {})
  };
}

function getMainMenuText(fromUser) {
  const userId = getUserId(fromUser);
  const user = Number.isFinite(userId) ? db.getUser(userId) : null;
  const source = fromUser && typeof fromUser === 'object' ? fromUser : {};

  const profile = {
    first_name: source.first_name || (user && user.first_name),
    last_name: source.last_name || (user && user.last_name),
    username: source.username || (user && user.username)
  };

  const subStatus = formatSubStatus(user);
  const nick = formatUserNick(profile);

  const conn =
    Number.isFinite(userId) && db.getBusinessConnectionByOwner
      ? db.getBusinessConnectionByOwner(userId)
      : null;

  const isConnected =
    conn && (conn.is_enabled === true || Number(conn.is_enabled) === 1);
  const connStatus = isConnected ? 'Подключён' : 'Не подключён';

  return (
    `<tg-emoji emoji-id="5994750571041525522">👋</tg-emoji> Привет, <b>${nick}</b>!\n` +
    `<tg-emoji emoji-id="5778423822940114949">🛡</tg-emoji> Статус подписки: <b>${subStatus}</b>\n` +
    `<tg-emoji emoji-id="5877495434124988415">📎</tg-emoji> Подключение: <b>${connStatus}</b>\n\n` +
    `<tg-emoji emoji-id="5875008416132370818">🔽</tg-emoji> Выберите нужный раздел ниже:`
  );
}

function getMainMenu(fromUser) {
  const userId = getUserId(fromUser);
  const text = getMainMenuText(fromUser);
  const keyboard = keys.getMainMenuKeyboard(userId);

  return buildMenu(text, keyboard, 'main');
}

function getSettingsMenu(userId) {
  const uid = getUserId(userId);
  const user = db.getUser(uid) || {
    notify_deleted: 1,
    notify_edited: 1
  };

  const delEnabled = Number(user.notify_deleted) === 1;
  const editEnabled = Number(user.notify_edited) === 1;

  const text =
    `<tg-emoji emoji-id="5877260593903175642">⚙️</tg-emoji> <b>Настройки</b>\n\n` +
    `Управляйте уведомлениями и функциями перехвата:\n\n` +
    `• Уведомления об удалённых: <b>${delEnabled ? 'Включены' : 'Выключены'}</b>\n` +
    `• Уведомления об изменённых: <b>${editEnabled ? 'Включены' : 'Выключены'}</b>\n` +
    `• Перехват сгорающих медиа (точка <code>.</code> или <code>,</code>): <b>Активен</b>\n`;

  const keyboard = keys.getSettingsKeyboard(uid);

  return buildMenu(text, keyboard, 'settings');
}

function normalizeArchiveTypes(types) {
  if (types instanceof Set) return types;
  if (Array.isArray(types)) return new Set(types);

  if (types && typeof types === 'object') {
    return new Set(
      Object.entries(types)
        .filter(([, enabled]) => Boolean(enabled))
        .map(([type]) => type)
    );
  }

  return new Set(Object.keys(ARCHIVE_TYPE_LABELS));
}

function getArchiveTypesMenu(chatIdOrSession, chatTitle, selectedTypesSet) {
  let chatId = chatIdOrSession;
  let title = chatTitle;
  let types = selectedTypesSet;

  if (chatIdOrSession && typeof chatIdOrSession === 'object') {
    chatId =
      chatIdOrSession.selectedChatId !== undefined
        ? chatIdOrSession.selectedChatId
        : chatIdOrSession.chatId;

    title =
      chatIdOrSession.selectedChatName ||
      chatIdOrSession.chatTitle ||
      chatTitle;

    types = chatIdOrSession.types || selectedTypesSet;
  }

  const typeSet = normalizeArchiveTypes(types);
  const typeKeys = Object.keys(ARCHIVE_TYPE_LABELS);
  const keyboard = new InlineKeyboard();

  for (let i = 0; i < typeKeys.length; i += 2) {
    for (let j = i; j < i + 2 && j < typeKeys.length; j++) {
      const type = typeKeys[j];
      const active = typeSet.has(type);
      const icon = active ? keys.EMOJI.check : keys.EMOJI.cross;

      keyboard.text({
        text: `${active ? '✅' : '❌'} ${ARCHIVE_TYPE_LABELS[type]}`,
        callback_data: `arch_toggle_${type}`,
        icon_custom_emoji_id: icon,
        style: active ? 'success' : 'primary'
      });
    }

    keyboard.row();
  }

  keyboard
    .text({
      text: 'Отменить экспорт',
      callback_data: 'arch_cancel',
      icon_custom_emoji_id: keys.EMOJI.cross,
      style: 'danger'
    })
    .text({
      text: 'Начать экспорт',
      callback_data: 'arch_start',
      icon_custom_emoji_id: keys.EMOJI.check,
      style: 'success'
    });

  const displayTitle =
    title || (chatId !== undefined && chatId !== null ? `Чат #${chatId}` : 'Чат');

  const displayChatId =
    chatId !== undefined && chatId !== null ? escapeHtml(chatId) : '—';

  const text =
    `<tg-emoji emoji-id="5877482652302315100">📁</tg-emoji> <b>Настройка экспорта чата</b>\n` +
    `<tg-emoji emoji-id="5877477244938489129">👤</tg-emoji> Собеседник: <b>${escapeHtml(displayTitle)}</b> (<code>${displayChatId}</code>)\n\n` +
    `<b>Выберите типы сообщений для экспорта:</b>`;

  return buildMenu(text, keyboard);
}

function getConnectMenu() {
  const username =
    botInfo && botInfo.username
      ? String(botInfo.username).replace(/^@/, '')
      : 'имя_бота';

  const text =
    `<tg-emoji emoji-id="5796440171364749940">📌</tg-emoji> <b>Подключение бота к Автоматизации</b>\n\n` +
    `Подключение выполняется владельцем аккаунта вручную. Бот не может подключить себя самостоятельно.\n\n` +
    `1. Откройте <b>Профиль</b> → <b>Изменить</b>.\n` +
    `2. Перейдите в раздел <b>Автоматизация чатов</b>.\n` +
    `3. Укажите юзернейм <code>@${escapeHtml(username)}</code> и нажмите <b>Добавить</b>.\n` +
    `4. Выдайте необходимые разрешения для чтения и управления сообщениями.\n\n`;

  const keyboard = keys.getConnectKeyboard();

  return buildMenu(text, keyboard);
}

function getSubMenu(userId) {
  const uid = getUserId(userId);
  const user = db.getUser(uid);
  const subStatus = formatSubStatus(user);

  const text =
    `<tg-emoji emoji-id="6028530359975548369">💎</tg-emoji> <b>Подписка</b>\n\n` +
    `Ваш текущий статус: <b>${subStatus}</b>\n\n` +
    `Выберите тариф для продления через <b>Звёзды</b>.\n`;

  const keyboard = keys.getSubKeyboard();

  return buildMenu(text, keyboard, 'subscription');
}

function getPremiumMenu(userId) {
  const uid = getUserId(userId);
  const isPremium = db.hasPremium(uid, ADMIN_IDS);

  const text =
    `<tg-emoji emoji-id="5879785854284599288">⭐️</tg-emoji> <b>Премиум — максимальный контроль</b>\n\n` +
    `Статус: <b>${isPremium ? 'Активен ✅' : 'Не подключён ❌'}</b>\n\n` +
    `Премиум оформляется на 365 дней.\n\n` +
    `Команды <code>.nah</code>, <code>.love</code>, <code>.bypass</code>, <code>.mute</code> и <code>.unmute</code> работают с обычной активной подпиской — Premium для них не требуется.`;

  const keyboard = keys.getPremiumKeyboard();

  return buildMenu(text, keyboard);
}

function getRefMenu(userId) {
  const uid = getUserId(userId);
  const user = db.getUser(uid) || { referrals_count: 0 };

  const username =
    botInfo && botInfo.username
      ? String(botInfo.username).replace(/^@/, '')
      : '';

  const refLink = `https://t.me/${username}?start=ref_${uid}`;

  const text =
    `<tg-emoji emoji-id="5877482652302315100">👥</tg-emoji> <b>Реферальная программа</b>\n\n` +
    `Приглашайте друзей и получайте бесплатную подписку!\n` +
    `• За каждого приглашённого друга вам начисляется: <b>+7 дней подписки</b>\n` +
    `• Ваш друг сразу получает бонус: <b>+3 дня подписки</b>\n\n` +
    `Приглашено пользователей: <b>${Number(user.referrals_count) || 0}</b>\n\n` +
    `Ваша реферальная ссылка:\n<code>${escapeHtml(refLink)}</code>`;

  const keyboard = keys.getRefKeyboard();

  return buildMenu(text, keyboard);
}

function getImportHistoryMenu() {
  const text =
    `<tg-emoji emoji-id="5877482652302315100">📥</tg-emoji> <b>Импорт истории чата</b>\n\n` +
    `Для импорта используйте команду <code>/archive</code> в личном чате с ботом, затем отправьте файл <code>result.json</code> или ZIP-архив экспорта Telegram Desktop.\n\n` +
    `Для создания HTML-экспорта используйте команду <code>/export</code> в личном чате с ботом.\n\n` +
    `<blockquote>JSON или ZIP отправляется сообщением в ответ на запрос бота после команды <code>/archive</code>.</blockquote>`;

  const keyboard = keys.getImportHistoryKeyboard();

  return buildMenu(text, keyboard);
}

function getAdminMenu() {
  const stats = db.getStats() || {};

  const text =
    `<tg-emoji emoji-id="5807868868886009920">👑</tg-emoji> <b>Великая панель</b>\n\n` +
    `<tg-emoji emoji-id="5931472654660800739">📊</tg-emoji> <b>Статистика:</b>\n` +
    `• Всего пользователей: <b>${Number(stats.totalUsers) || 0}</b>\n` +
    `• Активных подписок: <b>${Number(stats.activeSubs) || 0}</b>\n` +
    `• Премиум: <b>${Number(stats.premiumUsers) || 0}</b>\n` +
    `• Забанено пользователей: <b>${Number(stats.bannedUsers) || 0}</b>\n` +
    `• Активных подключений: <b>${Number(stats.activeConnections) || 0}</b>\n` +
    `<tg-emoji emoji-id="5875008416132370818">🔽</tg-emoji> Выберите нужный раздел ниже:`;

  const keyboard = keys.getAdminKeyboard();

  return buildMenu(text, keyboard, 'admin');
}

function getMenuBanner(type) {
  return MENU_BANNERS[type] || null;
}

function rememberBannerFileId(bannerKey, msg) {
  if (
    bannerKey &&
    !bannerFileIdCache[bannerKey] &&
    msg &&
    Array.isArray(msg.photo) &&
    msg.photo.length
  ) {
    bannerFileIdCache[bannerKey] = msg.photo[msg.photo.length - 1].file_id;
  }
}

function resolveApi(botOrApi) {
  if (botOrApi && typeof botOrApi.sendMessage === 'function') return botOrApi;

  if (
    botOrApi &&
    botOrApi.api &&
    typeof botOrApi.api.sendMessage === 'function'
  ) {
    return botOrApi.api;
  }

  return null;
}

function getBusinessConnectionForMenu(chatId) {
  if (typeof db.getBusinessConnectionByOwner !== 'function') return null;

  return db.getBusinessConnectionByOwner(chatId) || null;
}

function isConnectionEnabled(conn) {
  return Boolean(
    conn && (conn.is_enabled === true || Number(conn.is_enabled) === 1)
  );
}

function isSelfBusinessSend(conn, chatId) {
  return Boolean(conn && Number(conn.user_id) === Number(chatId));
}

async function trySendMenuPhoto(api, chatId, menu, sendOptions, bannerKey, errorTag) {
  const photoSource =
    bannerFileIdCache[bannerKey] || new InputFile(menu.banner);

  try {
    const msg = await api.sendPhoto(chatId, photoSource, {
      caption: menu.text,
      ...sendOptions
    });

    rememberBannerFileId(bannerKey, msg);
    return msg;
  } catch (error) {
    console.error(`[sendMenu ${errorTag} Photo Error]:`, error.message);
    return null;
  }
}

async function sendMenu(botOrApi, chatId, menu, options = {}) {
  if (!menu || typeof menu.text !== 'string') {
    throw new TypeError('Не передано меню с текстом');
  }

  const api = resolveApi(botOrApi);
  if (!api) {
    throw new TypeError('Не найден метод у объекта бота');
  }

  const conn = getBusinessConnectionForMenu(chatId);

  const isSelfSend = isSelfBusinessSend(conn, chatId);
  const isConnected = isConnectionEnabled(conn) && !isSelfSend;

  const sendOptions = {
    parse_mode: menu.parse_mode || 'HTML',
    ...options
  };

  const keyboard = menu.reply_markup || menu.keyboard;
  if (keyboard) {
    sendOptions.reply_markup = keyboard;
  }

  const bannerKey = menu.bannerKey;
  const bannerExists = menu.banner ? fs.existsSync(menu.banner) : false;

  if (isConnected && conn.connection_id) {
    sendOptions.business_connection_id = conn.connection_id;

    if (bannerExists) {
      const photoMsg = await trySendMenuPhoto(
        api,
        chatId,
        menu,
        sendOptions,
        bannerKey,
        'Business'
      );

      if (photoMsg) return photoMsg;
    }

    return api.sendMessage(chatId, menu.text, sendOptions);
  }

  if (bannerExists) {
    const photoMsg = await trySendMenuPhoto(
      api,
      chatId,
      menu,
      sendOptions,
      bannerKey,
      'Normal'
    );

    if (photoMsg) return photoMsg;
  }

  return api.sendMessage(chatId, menu.text, sendOptions);
}

async function replaceMenu(botOrApi, chatId, messageId, menu, options = {}) {
  const api = resolveApi(botOrApi);

  if (api && Number.isFinite(Number(messageId))) {
    try {
      await api.deleteMessage(chatId, messageId);
    } catch (error) {}
  }

  return sendMenu(botOrApi, chatId, menu, options);
}

module.exports = {
  ARCHIVE_TYPE_LABELS,
  MENU_BANNERS,
  bannerFileIdCache,

  isAdminUser,
  escapeHtml,
  formatUserNick,
  formatDateTime,
  formatShortDate,
  formatSubStatus,

  getMainMenuText,
  getMainMenu,
  getSettingsMenu,

  getArchiveEntryMenu: getImportHistoryMenu,
  getArchiveTypesMenu,

  getConnectMenu,
  getSubMenu,
  getPremiumMenu,
  getRefMenu,

  getImportHistoryMenu,
  getImportMenu: getImportHistoryMenu,

  getAdminMenu,
  getMenuBanner,

  sendMenu,
  replaceMenu
};
