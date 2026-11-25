'use strict';

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { Readable } = require('stream');
const { pipeline } = require('stream/promises');
const { InputFile, InlineKeyboard } = require('grammy');
const config = require('./config');

const {
  bot,
  botInfo,
  ADMIN_IDS,
  SUB_PRICES,
  TEMP_DIR,
  MEDIA_DIR,
  adminStates,
  userStates,
  archiveSessions
} = config;

const api = bot.api;
const db = require('./database');
const menus = require('./menus');
const archive = require('./archive');
const business = require('./business');
const keys = require('./menuKeys');

const NO_PREVIEW = { link_preview_options: { is_disabled: true } };
const welcomeBonusGranted = new Set();

function sendHtml(chatId, text, extra = {}) {
  return api.sendMessage(chatId, text, { parse_mode: 'HTML', ...extra });
}

async function showMenu(chatId, messageId, menu) {
  if (messageId) {
    return menus.replaceMenu(bot, chatId, messageId, menu);
  }
  return menus.sendMenu(bot, chatId, menu);
}

function cleanFilePath(value) {
  if (!value) return null;
  if (typeof value === 'string') return value;
  if (typeof value === 'object') {
    if (typeof value.filePath === 'string') return value.filePath;
    if (typeof value.path === 'string') return value.path;
    if (typeof value.file_path === 'string') return value.file_path;
  }
  return null;
}

function escapeHtml(value) {
  if (typeof menus.escapeHtml === 'function') return menus.escapeHtml(value);
  if (typeof archive.escapeHtml === 'function') return archive.escapeHtml(value);
  if (value === null || value === undefined) return '';
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

function isAdmin(userId) {
  return typeof menus.isAdminUser === 'function'
    ? menus.isAdminUser(userId)
    : (ADMIN_IDS || []).some((id) => String(id) === String(userId));
}

function isEnabled(value) {
  return value === true || value === 1 || value === '1';
}

function getUser(userId) {
  try {
    return db.getUser(userId) || null;
  } catch (_) {
    return null;
  }
}

function isBanned(user) {
  return Boolean(user && (user.is_banned === true || Number(user.is_banned) === 1));
}

function getSubscriptionExpiry(user) {
  if (!user) return null;
  const keysList = [
    'subscription_until',
    'sub_until',
    'subscription_expires_at',
    'sub_expires_at',
    'subscription_end',
    'subscription_expiry',
    'sub_expiry',
    'expires_at'
  ];
  for (const key of keysList) {
    if (user[key] === null || user[key] === undefined || user[key] === '') continue;
    if (user[key] instanceof Date) return Math.floor(user[key].getTime() / 1000);
    const numeric = Number(user[key]);
    if (Number.isFinite(numeric)) return Math.floor(numeric > 1000000000000 ? numeric / 1000 : numeric);
    const parsed = Date.parse(String(user[key]));
    if (Number.isFinite(parsed)) return Math.floor(parsed / 1000);
  }
  return null;
}

function hasActiveSubscriptionForBonus(userId, user) {
  const expiry = getSubscriptionExpiry(user);
  if (expiry !== null) return expiry > Math.floor(Date.now() / 1000);
  if (user && Number(user.is_premium) === 1) return false;
  if (typeof db.isSubActive === 'function') {
    try {
      return Boolean(db.isSubActive(userId));
    } catch (_) {}
  }
  return false;
}

function setTrialUsed(userId) {
  if (typeof db.setTrialUsed === 'function') return db.setTrialUsed(userId, 1);
  if (typeof db.markTrialUsed === 'function') return db.markTrialUsed(userId);
  if (typeof db.updateUserSetting === 'function') return db.updateUserSetting(userId, 'trial_used', 1);
  if (typeof db.updateUser === 'function') return db.updateUser(userId, { trial_used: 1 });
  return false;
}

function addSubscriptionDays(userId, days, isPremium = false) {
  if (typeof db.addSubscription === 'function') return db.addSubscription(userId, days, isPremium);
  if (typeof db.addSubscriptionDays === 'function') return db.addSubscriptionDays(userId, days);
  throw new Error('В database.js отсутствует метод выдачи подписки.');
}

function grantPremium(userId) {
  if (typeof db.setPremium === 'function') return db.setPremium(userId, 1);
  return addSubscriptionDays(userId, 365, true);
}

function hasReferrer(user) {
  if (!user) return false;
  const referrer = user.referred_by ?? user.referrer_id ?? user.referral_id ?? user.invited_by;
  return Boolean(referrer && Number(referrer) > 0);
}

function maybeGrantWelcomeBonus(userId, user, isNewUser = false) {
  const key = String(userId);
  if (welcomeBonusGranted.has(key)) return false;
  if (user && (user.trial_used === true || Number(user.trial_used) === 1)) return false;
  if (!isNewUser && (!user || Number(user.trial_used) !== 0 || hasReferrer(user))) return false;
  if (hasActiveSubscriptionForBonus(userId, user)) {
    if (isNewUser) {
      welcomeBonusGranted.add(key);
      setTrialUsed(userId);
    }
    return false;
  }
  try {
    if (typeof db.activateTrial === 'function') {
      db.activateTrial(userId, 3);
    } else {
      addSubscriptionDays(userId, 3, false);
      setTrialUsed(userId);
    }
    welcomeBonusGranted.add(key);
    return true;
  } catch (error) {
    console.error('[Welcome Bonus Error]:', error);
    return false;
  }
}

function createOrGetUser(fromUser, referredBy = null) {
  if (!fromUser || !fromUser.id) return { user: null, isNew: false, referrerId: null };
  const userId = fromUser.id;
  const existing = getUser(userId);
  if (existing) {
    maybeGrantWelcomeBonus(userId, existing, false);
    return { user: getUser(userId) || existing, isNew: false, referrerId: null };
  }
  if (typeof db.createUser !== 'function') throw new Error('В database.js отсутствует метод createUser().');
  const result = db.createUser({ userId, username: fromUser.username || '', firstName: fromUser.first_name || '', referredBy: referredBy || null });
  const isNew = result && typeof result.isNew === 'boolean' ? result.isNew : true;
  const user = (result && result.user) || getUser(userId);
  const savedReferrerId = (result && result.referrerId) || referredBy || null;
  if (isNew && referredBy) {
    welcomeBonusGranted.add(String(userId));
    setTrialUsed(userId);
  } else if (isNew) {
    maybeGrantWelcomeBonus(userId, user, true);
  }
  return { user: getUser(userId) || user, isNew, referrerId: isNew ? savedReferrerId : null };
}

function ensureUserRecord(fromUser) {
  return createOrGetUser(fromUser).user;
}

async function editOrSend(chatId, messageId, text, replyMarkup) {
  const options = { parse_mode: 'HTML', ...NO_PREVIEW };
  if (replyMarkup) options.reply_markup = replyMarkup;
  try {
    return await api.editMessageText(chatId, messageId, text, options);
  } catch (error) {
    const errorText = String((error && (error.description || error.message)) || error).toLowerCase();
    if (errorText.includes('message is not modified')) return null;
    return api.sendMessage(chatId, text, options);
  }
}

async function presentMessage(chatId, messageId, text, replyMarkup) {
  if (messageId) return editOrSend(chatId, messageId, text, replyMarkup);
  const options = { parse_mode: 'HTML', ...NO_PREVIEW };
  if (replyMarkup) options.reply_markup = replyMarkup;
  return api.sendMessage(chatId, text, options);
}

function normalizeChatId(value) {
  if (value === null || value === undefined) return '';
  return String(value);
}

async function collectAvailableChats(userId) {
  const chatMap = new Map();
  if (typeof db.getOwnerChats === 'function') {
    const dbChats = db.getOwnerChats(userId) || [];
    for (const chat of dbChats) {
      const chatId = normalizeChatId(chat.chatId ?? chat.chat_id);
      if (!chatId) continue;
      chatMap.set(chatId, {
        chat_id: chatId,
        title: chat.peerName || chat.chat_title || chat.first_name || chat.peerUsername || chat.username || `Чат ${chatId}`,
        username: chat.peerUsername || chat.username || null,
        count: Number(chat.msgCount ?? chat.msg_count ?? 0)
      });
    }
  }
  return Array.from(chatMap.values());
}

function normalizeArchiveType(value) {
  const type = String(value || 'text').toLowerCase();
  if (type === 'voice_message') return 'voice';
  if (type === 'round_video' || type === 'video_message') return 'video_note';
  if (type === 'video_file') return 'video';
  if (type === 'audio_file') return 'audio';
  if (type === 'gif') return 'animation';
  return type;
}

function toBooleanFlag(value) {
  if (value === true) return true;
  if (value === false || value === null || value === undefined) return false;
  return String(value).toLowerCase() === '1' || String(value).toLowerCase() === 'true';
}

function normalizeDbArchiveMessage(row, userId, index) {
  const fromId = row.from_id ?? row.sender_id ?? row.fromId;
  const rowOwnerId = row.owner_id ?? row.ownerId ?? userId;
  let outgoing;
  if (row.is_outgoing !== null && row.is_outgoing !== undefined) {
    outgoing = toBooleanFlag(row.is_outgoing);
  } else {
    outgoing = fromId !== null && fromId !== undefined && String(fromId).replace(/^user/i, '') === String(rowOwnerId).replace(/^user/i, '');
  }
  return {
    ...row,
    id: row.id ?? row.message_id ?? index + 1,
    message_id: row.message_id ?? row.id ?? index + 1,
    date: row.date ?? row.created_at ?? 0,
    from_id: fromId,
    owner_id: rowOwnerId,
    from_name: row.from_name || row.sender_name || '',
    is_outgoing: outgoing,
    text: row.text || row.caption || '',
    type: normalizeArchiveType(row.type || row.media_type || 'text'),
    media_type: normalizeArchiveType(row.media_type || row.type || 'text'),
    media_path: cleanFilePath(row.media_path) || cleanFilePath(row.local_path) || cleanFilePath(row.file_path)
  };
}

function filterArchiveMessages(messages, allowedTypes) {
  const allowed = new Set(Array.from(allowedTypes || []).map(normalizeArchiveType));
  return (messages || []).filter((message) => {
    const type = normalizeArchiveType(message.type || message.media_type || 'text');
    return allowed.has(type);
  });
}

async function generateArchiveFile(userId, targetChatId, targetChatTitle, allowedTypesSet) {
  const allowedTypes = Array.from(allowedTypesSet || Object.keys(menus.ARCHIVE_TYPE_LABELS || {}));
  let dbMessages = [];
  if (typeof db.getChatMessages === 'function') dbMessages = db.getChatMessages(targetChatId, userId) || [];
  let messages = dbMessages.map((row, index) => normalizeDbArchiveMessage(row, userId, index));
  messages = filterArchiveMessages(messages, allowedTypes);
  const chatTitle = targetChatTitle || `Чат ${targetChatId}`;
  const html = archive.buildChatHtmlArchive(chatTitle, targetChatId, messages);
  const outputDirectory = path.resolve(TEMP_DIR || path.join(__dirname, 'temp'));
  fs.mkdirSync(outputDirectory, { recursive: true });
  const safeChatId = String(targetChatId).replace(/[^\w.-]/g, '_');
  const filePath = path.join(outputDirectory, `archive_${safeChatId}_${Date.now()}.html`);
  fs.writeFileSync(filePath, html, 'utf8');
  return { filePath, count: messages.length, chatTitle };
}

async function showExportChatSelection(chatId, messageId, userId) {
  const chats = await collectAvailableChats(userId);
  if (!chats.length) {
    const text = `<tg-emoji emoji-id="5877482652302315100">📁</tg-emoji> <b>Список чатов пуст</b>\n\nВ базе пока нет сохранённых переписок.\nПодключите бота в <b>Telegram для бизнеса → Чат-боты</b> или импортируйте историю через <code>/archive</code>.`;
    const keyboard = new InlineKeyboard()
      .text({ text: 'Подключить бота', callback_data: 'menu_connect', icon_custom_emoji_id: keys.EMOJI.user, style: 'primary' })
      .row()
      .text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
    return presentMessage(chatId, messageId, text, keyboard);
  }
  const keyboard = new InlineKeyboard();
  for (const chat of chats.slice(0, 35)) {
    const label = chat.username ? `${chat.title} (@${chat.username})` : chat.title;
    keyboard.text({ text: String(label).slice(0, 48), callback_data: `arch_sel_${chat.chat_id}`, icon_custom_emoji_id: keys.EMOJI.user, style: 'primary' }).row();
  }
  keyboard.text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
  return presentMessage(chatId, messageId, `<tg-emoji emoji-id="5877482652302315100">📁</tg-emoji> <b>Выберите чат для создания HTML-архива:</b>\n\nНайдено диалогов: <b>${chats.length}</b>`, keyboard);
}

async function startExportForChat(chatId, messageId, userId, targetChat) {
  const session = {
    step: 'configure_types',
    selectedChatId: String(targetChat.chat_id),
    selectedChatName: targetChat.title || `Чат ${targetChat.chat_id}`,
    types: new Set(Object.keys(menus.ARCHIVE_TYPE_LABELS || {}))
  };
  archiveSessions.set(userId, session);
  const typesMenu = menus.getArchiveTypesMenu(session);
  return presentMessage(chatId, messageId, typesMenu.text, typesMenu.reply_markup);
}

function findEndOfCentralDirectory(buffer) {
  const minimumOffset = Math.max(0, buffer.length - 22 - 0xffff);
  for (let offset = buffer.length - 22; offset >= minimumOffset; offset--) {
    if (offset + 4 <= buffer.length && buffer.readUInt32LE(offset) === 0x06054b50) return offset;
  }
  return -1;
}

function safeZipDestination(rootDirectory, entryName) {
  const normalizedName = String(entryName || '').replace(/\\/g, '/');
  if (!normalizedName || normalizedName.includes('\0') || normalizedName.startsWith('/') || /^[a-zA-Z]:/.test(normalizedName)) return null;
  const parts = normalizedName.split('/').filter(Boolean);
  if (parts.some((part) => part === '.' || part === '..')) return null;
  const destination = path.resolve(rootDirectory, ...parts);
  const relative = path.relative(rootDirectory, destination);
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) return null;
  return { path: destination, isDirectory: normalizedName.endsWith('/') };
}

function extractTelegramDesktopZip(zipPath, outputDirectory) {
  const zipBuffer = fs.readFileSync(zipPath);
  const eocdOffset = findEndOfCentralDirectory(zipBuffer);
  if (eocdOffset < 0) throw new Error('ZIP-архив повреждён или не поддерживается.');
  const diskNumber = zipBuffer.readUInt16LE(eocdOffset + 4);
  const centralDirectoryDisk = zipBuffer.readUInt16LE(eocdOffset + 6);
  const entriesOnDisk = zipBuffer.readUInt16LE(eocdOffset + 8);
  const totalEntries = zipBuffer.readUInt16LE(eocdOffset + 10);
  const centralDirectorySize = zipBuffer.readUInt32LE(eocdOffset + 12);
  const centralDirectoryOffset = zipBuffer.readUInt32LE(eocdOffset + 16);
  if (diskNumber !== 0 || centralDirectoryDisk !== 0 || entriesOnDisk !== totalEntries) throw new Error('Многотомные ZIP-архивы не поддерживаются.');
  if (totalEntries === 0xffff || centralDirectorySize === 0xffffffff || centralDirectoryOffset === 0xffffffff) throw new Error('ZIP64-архивы не поддерживаются.');
  if (centralDirectoryOffset + centralDirectorySize > eocdOffset || centralDirectoryOffset + centralDirectorySize > zipBuffer.length) throw new Error('Повреждённая таблица ZIP-архива.');
  fs.mkdirSync(outputDirectory, { recursive: true });
  let offset = centralDirectoryOffset;
  let totalUncompressedBytes = 0;
  const maxEntryBytes = 150 * 1024 * 1024;
  const maxTotalBytes = 500 * 1024 * 1024;
  for (let index = 0; index < totalEntries; index++) {
    if (offset + 46 > zipBuffer.length || zipBuffer.readUInt32LE(offset) !== 0x02014b50) throw new Error('Повреждённая запись в ZIP-архиве.');
    const flags = zipBuffer.readUInt16LE(offset + 8);
    const compressionMethod = zipBuffer.readUInt16LE(offset + 10);
    const compressedSize = zipBuffer.readUInt32LE(offset + 20);
    const uncompressedSize = zipBuffer.readUInt32LE(offset + 24);
    const fileNameLength = zipBuffer.readUInt16LE(offset + 28);
    const extraLength = zipBuffer.readUInt16LE(offset + 30);
    const commentLength = zipBuffer.readUInt16LE(offset + 32);
    const localHeaderOffset = zipBuffer.readUInt32LE(offset + 42);
    const nameStart = offset + 46;
    const nameEnd = nameStart + fileNameLength;
    if (nameEnd > zipBuffer.length) throw new Error('Повреждённое имя файла в ZIP-архиве.');
    const entryName = zipBuffer.subarray(nameStart, nameEnd).toString('utf8');
    offset = nameEnd + extraLength + commentLength;
    if (offset > zipBuffer.length) throw new Error('Повреждённая запись центрального каталога ZIP.');
    if (flags & 0x0001) throw new Error('Зашифрованные ZIP-архивы не поддерживаются.');
    const destination = safeZipDestination(outputDirectory, entryName);
    if (!destination) continue;
    if (destination.isDirectory) {
      fs.mkdirSync(destination.path, { recursive: true });
      continue;
    }
    if (uncompressedSize > maxEntryBytes) throw new Error('Файл в ZIP-архиве превышает допустимый размер.');
    totalUncompressedBytes += uncompressedSize;
    if (totalUncompressedBytes > maxTotalBytes) throw new Error('Распакованный ZIP-архив превышает допустимый размер.');
    if (localHeaderOffset + 30 > zipBuffer.length || zipBuffer.readUInt32LE(localHeaderOffset) !== 0x04034b50) throw new Error('Повреждённый заголовок файла в ZIP-архиве.');
    const localFileNameLength = zipBuffer.readUInt16LE(localHeaderOffset + 26);
    const localExtraLength = zipBuffer.readUInt16LE(localHeaderOffset + 28);
    const dataStart = localHeaderOffset + 30 + localFileNameLength + localExtraLength;
    const dataEnd = dataStart + compressedSize;
    if (dataStart < 0 || dataEnd > zipBuffer.length) throw new Error('Повреждённые данные файла в ZIP-архиве.');
    const compressedData = zipBuffer.subarray(dataStart, dataEnd);
    let fileData;
    if (compressionMethod === 0) {
      fileData = Buffer.from(compressedData);
    } else if (compressionMethod === 8) {
      fileData = zlib.inflateRawSync(compressedData, { maxOutputLength: Math.max(uncompressedSize + 1, 1) });
    } else {
      throw new Error(`Метод сжатия ZIP ${compressionMethod} не поддерживается.`);
    }
    if (fileData.length !== uncompressedSize) throw new Error('Размер распакованного файла не совпадает с ZIP-записью.');
    fs.mkdirSync(path.dirname(destination.path), { recursive: true });
    fs.writeFileSync(destination.path, fileData);
  }
  return outputDirectory;
}

function findResultJson(directory) {
  const entries = fs.readdirSync(directory, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isFile() && entry.name.toLowerCase() === 'result.json') return entryPath;
    if (entry.isDirectory()) {
      const nested = findResultJson(entryPath);
      if (nested) return nested;
    }
  }
  return null;
}

async function removeDirectory(directory) {
  if (!directory) return;
  try {
    await fs.promises.rm(directory, { recursive: true, force: true });
  } catch (_) {}
}

async function downloadFileToDisk(fileId, destinationPath, maxBytes) {
  let file;
  try {
    file = await api.getFile(fileId);
  } catch (error) {
    const description = String((error && (error.description || error.message)) || '');
    if (/file is too big/i.test(description)) {
      throw new Error('Bot API не позволяет скачивать файлы больше 20 МБ. Экспортируйте историю без медиа или поднимите локальный Bot API сервер.');
    }
    throw error;
  }
  if (!file || !file.file_path) throw new Error('Telegram не вернул путь к файлу.');
  if (file.file_size && file.file_size > maxBytes) throw new Error('Файл слишком большой.');
  if (path.isAbsolute(file.file_path) && fs.existsSync(file.file_path)) {
    await fs.promises.copyFile(file.file_path, destinationPath);
    return destinationPath;
  }
  const url = `https://api.telegram.org/file/bot${config.BOT_TOKEN}/${encodeURI(file.file_path)}`;
  const response = await fetch(url);
  if (!response.ok || !response.body) throw new Error(`Ошибка загрузки файла: HTTP ${response.status}`);
  await pipeline(Readable.fromWeb(response.body), fs.createWriteStream(destinationPath));
  return destinationPath;
}

async function importArchiveDocument(message, userId) {
  const chatId = message.chat.id;
  const fileName = String(message.document && message.document.file_name ? message.document.file_name : '').toLowerCase();
  const mimeType = String(message.document.mime_type || '').toLowerCase();
  const isZip = fileName.endsWith('.zip') || mimeType === 'application/zip';
  const isJson = fileName.endsWith('.json') || mimeType === 'application/json';
  if (!isZip && !isJson) {
    return sendHtml(chatId, 'Отправьте JSON или ZIP-архив экспорта Telegram Desktop. Для начала импорта используйте команду /archive.');
  }
  const fileSize = Number(message.document.file_size || 0);
  const maxImportBytes = Number(config.MAX_IMPORT_FILE_MB || 200) * 1024 * 1024;
  if (fileSize > maxImportBytes) {
    userStates.delete(userId);
    return sendHtml(chatId, `Размер файла превышает допустимый предел ${Number(config.MAX_IMPORT_FILE_MB || 200)} МБ.`);
  }
  const tempRoot = path.join(path.resolve(TEMP_DIR || path.join(__dirname, 'temp')), `import_${userId}_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`);
  fs.mkdirSync(tempRoot, { recursive: true });
  const waitMessage = await sendHtml(chatId, '📥 Скачивание и импорт истории переписки...');
  try {
    const downloadedPath = await downloadFileToDisk(message.document.file_id, path.join(tempRoot, isZip ? 'import.zip' : 'result.json'), maxImportBytes);
    if (!downloadedPath || !fs.existsSync(downloadedPath)) throw new Error('Не удалось скачать файл импорта.');
    let jsonPath = downloadedPath;
    if (isZip) {
      const extractedDirectory = path.join(tempRoot, 'extracted');
      extractTelegramDesktopZip(downloadedPath, extractedDirectory);
      jsonPath = findResultJson(extractedDirectory);
      if (!jsonPath) throw new Error('В ZIP-архиве не найден файл result.json.');
    }
    const importedCount = await archive.importTelegramDesktopExport(jsonPath, db, userId, { mediaDir: MEDIA_DIR || path.join(__dirname, 'media'), maxMediaBytes: Number(config.MAX_IMPORTED_MEDIA_MB || 100) * 1024 * 1024 });
    userStates.delete(userId);
    const keyboard = new InlineKeyboard().text({ text: 'Назад в меню', callback_data: 'back_main', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
    return editOrSend(chatId, waitMessage.message_id, `<tg-emoji emoji-id="5776375003280838798">✅</tg-emoji> <b>Импорт переписки успешно завершён!</b>\n\n• Добавлено сообщений: <b>${Number(importedCount) || 0}</b>\n\nДля создания HTML-архива используйте команду <code>/export</code>.`, keyboard);
  } catch (error) {
    userStates.delete(userId);
    const keyboard = new InlineKeyboard()
      .text({ text: 'Повторить импорт', callback_data: 'archive_import_again', style: 'primary' }).row()
      .text({ text: 'Назад в меню', callback_data: 'back_main', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
    return editOrSend(chatId, waitMessage.message_id, `<tg-emoji emoji-id="5778527486270770928">❌</tg-emoji> <b>Ошибка импорта:</b> <code>${escapeHtml(error.message)}</code>`, keyboard);
  } finally {
    await removeDirectory(tempRoot);
  }
}

function containsHtmlMarkup(text) {
  return /<\s*\/?\s*(?:b|strong|i|em|u|ins|s|strike|del|span|tg-spoiler|a|code|pre|blockquote|tg-emoji)\b[^>]*>/i.test(String(text || ''));
}

function hasMessageEntities(entities) {
  return Array.isArray(entities) && entities.length > 0;
}

async function updateUserNotification(userId, field, value) {
  if (field === 'notify_deleted' && typeof db.setNotifyDeleted === 'function') { db.setNotifyDeleted(userId, value); return true; }
  if (field === 'notify_edited' && typeof db.setNotifyEdited === 'function') { db.setNotifyEdited(userId, value); return true; }
  if (typeof db.updateUser === 'function') { db.updateUser(userId, { [field]: value }); return true; }
  return false;
}

async function sendHtmlBroadcastText(targetUserId, text) {
  try {
    return await api.sendMessage(targetUserId, text, { parse_mode: 'HTML', ...NO_PREVIEW });
  } catch (_) {
    return api.sendMessage(targetUserId, escapeHtml(text), { parse_mode: 'HTML', ...NO_PREVIEW });
  }
}

async function sendHtmlBroadcastMedia(targetUserId, message) {
  const caption = message.caption || '';
  const sendWithFallback = async (sendFunction) => {
    try {
      return await sendFunction({ caption, parse_mode: 'HTML' });
    } catch (_) {
      return sendFunction({ caption: escapeHtml(caption), parse_mode: 'HTML' });
    }
  };
  if (message.photo && message.photo.length) {
    const photo = message.photo[message.photo.length - 1];
    return sendWithFallback((options) => api.sendPhoto(targetUserId, photo.file_id, options));
  }
  if (message.video) return sendWithFallback((options) => api.sendVideo(targetUserId, message.video.file_id, options));
  if (message.animation) return sendWithFallback((options) => api.sendAnimation(targetUserId, message.animation.file_id, options));
  if (message.audio) return sendWithFallback((options) => api.sendAudio(targetUserId, message.audio.file_id, options));
  if (message.voice) return sendWithFallback((options) => api.sendVoice(targetUserId, message.voice.file_id, options));
  if (message.document) return sendWithFallback((options) => api.sendDocument(targetUserId, message.document.file_id, options));
  return api.copyMessage(targetUserId, message.chat.id, message.message_id);
}

async function sendBroadcastMessage(targetUserId, message) {
  if (typeof message.text === 'string') {
    if (hasMessageEntities(message.entities)) return api.sendMessage(targetUserId, message.text, { entities: message.entities, ...NO_PREVIEW });
    if (containsHtmlMarkup(message.text)) return sendHtmlBroadcastText(targetUserId, message.text);
    return api.sendMessage(targetUserId, message.text, NO_PREVIEW);
  }
  if (typeof message.caption === 'string' && !hasMessageEntities(message.caption_entities) && containsHtmlMarkup(message.caption)) {
    return sendHtmlBroadcastMedia(targetUserId, message);
  }
  return api.copyMessage(targetUserId, message.chat.id, message.message_id);
}

async function handleSuccessfulPayment(message) {
  const payment = message && message.successful_payment;
  if (!payment) return;
  const userId = message.from.id;
  ensureUserRecord(message.from);
  const payload = String(payment.invoice_payload || '');
  const match = payload.match(/^sub_(7|30|90|premium)(?:_|$)/);
  let days = 30;
  let isPremium = false;
  if (match) {
    if (match[1] === '7') days = 7;
    else if (match[1] === '30') days = 30;
    else if (match[1] === '90') days = 90;
    else if (match[1] === 'premium') { days = 365; isPremium = true; }
  }
  try {
    if (isPremium) grantPremium(userId);
    else addSubscriptionDays(userId, days, false);
  } catch (error) {
    console.error('[Subscription Payment Error]:', error);
    return sendHtml(message.chat.id, `<tg-emoji emoji-id="5778527486270770928">❌</tg-emoji> Оплата получена, но подписку не удалось обновить. Обратитесь к администратору.`);
  }
  const mainMenu = menus.getMainMenu(message.from);
  return menus.sendMenu(bot, message.chat.id, mainMenu);
}

business.registerBusinessHandlers();

bot.on('pre_checkout_query', async (ctx) => {
  try {
    await ctx.answerPreCheckoutQuery(true);
  } catch (error) {
    console.error('[PreCheckout Error]:', error);
  }
});

bot.on('message:successful_payment', async (ctx) => {
  try {
    await handleSuccessfulPayment(ctx.message);
  } catch (error) {
    console.error('[Payment Error]:', error);
  }
});

bot.command('start', async (ctx) => {
  try {
    const message = ctx.message;
    if (!message.chat || message.chat.type !== 'private') return;
    const userId = message.from.id;
    const existing = getUser(userId);
    const payload = ctx.match ? String(ctx.match).trim() : '';
    const referralMatch = payload.match(/^ref_(\d+)$/);
    let referredBy = null;
    if (!existing && referralMatch) {
      const referrerId = Number(referralMatch[1]);
      const referrer = referrerId ? getUser(referrerId) : null;
      if (referrer && referrerId !== Number(userId) && !isBanned(referrer)) referredBy = referrerId;
    }
    const registration = createOrGetUser(message.from, referredBy);
    const user = registration.user;
    if (isBanned(user) && !isAdmin(userId)) {
      return sendHtml(message.chat.id, `<tg-emoji emoji-id="5778527486270770928">🚫</tg-emoji> <b>Ваш доступ к боту заблокирован администратором.</b>`);
    }
    if (registration.isNew && registration.referrerId) {
      const referrerId = registration.referrerId;
      sendHtml(referrerId, `<tg-emoji emoji-id="5877482652302315100">🎉</tg-emoji> По вашей реферальной ссылке зарегистрировался <b>${menus.formatUserNick(message.from)}</b>!\n<tg-emoji emoji-id="5776375003280838798">✅</tg-emoji> Вам начислено <b>+7 дней подписки</b>!`).catch(() => {});
      sendHtml(message.chat.id, `<tg-emoji emoji-id="5879785854284599288">🎁</tg-emoji> Вы перешли по приглашению друга и получили бонус: <b>+3 дня подписки</b>!`).catch(() => {});
    }
    const mainMenu = menus.getMainMenu(message.from);
    return menus.sendMenu(bot, message.chat.id, mainMenu);
  } catch (error) {
    console.error('[Start Error]:', error);
  }
});

bot.command('export', async (ctx) => {
  const message = ctx.message;
  try {
    if (!message.chat || message.chat.type !== 'private') return;
    const userId = message.from.id;
    const user = ensureUserRecord(message.from);
    if (isBanned(user) && !isAdmin(userId)) return sendHtml(message.chat.id, '🚫 Ваш аккаунт заблокирован.');
    const directChatId = ctx.match ? String(ctx.match).trim() : null;
    if (directChatId) {
      const chats = await collectAvailableChats(userId);
      const selected = chats.find((chat) => String(chat.chat_id) === String(directChatId));
      if (!selected) return sendHtml(message.chat.id, 'Этот чат не найден среди ваших доступных переписок. Используйте /export, чтобы выбрать чат из списка.');
      return startExportForChat(message.chat.id, null, userId, selected);
    }
    return showExportChatSelection(message.chat.id, null, userId);
  } catch (error) {
    console.error('[/export Error]:', error);
    return sendHtml(message.chat.id, `<tg-emoji emoji-id="5778527486270770928">❌</tg-emoji> Не удалось открыть экспорт: <code>${escapeHtml(error.message)}</code>`).catch(() => {});
  }
});

bot.command('archive', async (ctx) => {
  try {
    const message = ctx.message;
    if (!message.chat || message.chat.type !== 'private') return;
    const userId = message.from.id;
    const user = ensureUserRecord(message.from);
    if (isBanned(user) && !isAdmin(userId)) return sendHtml(message.chat.id, '🚫 Ваш аккаунт заблокирован.');
    userStates.set(userId, { action: 'archive_wait_file' });
    const keyboard = new InlineKeyboard().text({ text: 'Отмена', callback_data: 'archive_import_cancel', style: 'danger' });
    return sendHtml(message.chat.id, `<tg-emoji emoji-id="5877482652302315100">📥</tg-emoji> <b>Импорт истории Telegram Desktop</b>\n\nОтправьте следующим сообщением файл <code>result.json</code> или ZIP-архив экспорта Telegram Desktop.`, { reply_markup: keyboard });
  } catch (error) {
    console.error('[/archive Error]:', error);
  }
});

bot.command('admin', async (ctx) => {
  try {
    const message = ctx.message;
    if (!message.chat || message.chat.type !== 'private') return;
    if (!isAdmin(message.from.id)) return sendHtml(message.chat.id, '⛔️ У вас нет прав администратора.');
    const adminMenu = menus.getAdminMenu();
    return menus.sendMenu(bot, message.chat.id, adminMenu);
  } catch (error) {
    console.error('[/admin Error]:', error);
  }
});

bot.on('inline_query', async (ctx) => {
  try {
    const userId = ctx.from.id;
    const username = botInfo.username || '';
    const refLink = `https://t.me/${username}?start=ref_${userId}`;
    const results = [{
      type: 'article',
      id: `ref_${userId}`,
      title: 'Пригласить друга и получить +7 дней подписки',
      description: 'Друг получит +3 дня при первой регистрации',
      input_message_content: {
        message_text: `<tg-emoji emoji-id="5994750571041525522">👋</tg-emoji> Привет! Я пользуюсь бизнес-ботом для перехвата удалённых, изменённых и сгорающих сообщений в Telegram.\n\n<tg-emoji emoji-id="5879785854284599288">🎁</tg-emoji> Переходи по моей ссылке и забирай <b>+3 дня подписки бесплатно</b>:\n${refLink}`,
        parse_mode: 'HTML'
      },
      reply_markup: new InlineKeyboard().url({ text: '🚀 Подключить бота (+3 дня)', url: refLink })
    }];
    await ctx.answerInlineQuery(results, { cache_time: 5, is_personal: true });
  } catch (_) {}
});

bot.on('callback_query:data', async (ctx) => {
  const query = ctx.callbackQuery;
  if (!query.message) return ctx.answerCallbackQuery().catch(() => {});
  const chatId = query.message.chat.id;
  const messageId = query.message.message_id;
  const userId = query.from.id;
  const data = query.data || '';
  try {
    const user = ensureUserRecord(query.from);
    if (isBanned(user) && !isAdmin(userId)) {
      return ctx.answerCallbackQuery({ text: '🚫 Ваш доступ к боту заблокирован.', show_alert: true });
    }
    if (data === 'back_main') {
      userStates.delete(userId);
      adminStates.delete(userId);
      const mainMenu = menus.getMainMenu(query.from);
      await showMenu(chatId, messageId, mainMenu);
      return ctx.answerCallbackQuery();
    }
    if (data === 'menu_settings') {
      userStates.delete(userId);
      const settingsMenu = menus.getSettingsMenu(userId);
      await showMenu(chatId, messageId, settingsMenu);
      return ctx.answerCallbackQuery();
    }
    if (data === 'toggle_deleted' || data === 'toggle_edited') {
      const field = data === 'toggle_deleted' ? 'notify_deleted' : 'notify_edited';
      const currentUser = getUser(userId) || {};
      const currentValue = currentUser[field] === undefined ? 1 : Number(currentUser[field]);
      const nextValue = currentValue ? 0 : 1;
      const updated = await updateUserNotification(userId, field, nextValue);
      if (!updated) return ctx.answerCallbackQuery({ text: 'В database.js отсутствует настройка.', show_alert: true });
      const settingsMenu = menus.getSettingsMenu(userId);
      await showMenu(chatId, messageId, settingsMenu);
      return ctx.answerCallbackQuery({ text: `${field === 'notify_deleted' ? 'Удалённые' : 'Изменённые'}: ${nextValue ? 'ВКЛ ✅' : 'ВЫКЛ ❌'}` });
    }
    if (data === 'menu_connect') {
      const connectMenu = menus.getConnectMenu();
      await showMenu(chatId, messageId, connectMenu);
      return ctx.answerCallbackQuery();
    }
    if (data === 'check_connection') {
      const connection = typeof db.getBusinessConnectionByOwner === 'function' ? db.getBusinessConnectionByOwner(userId) : null;
      if (connection && isEnabled(connection.is_enabled)) {
        await ctx.answerCallbackQuery({ text: '✅ Бизнес-режим успешно подключён и активен!', show_alert: true });
        const mainMenu = menus.getMainMenu(query.from);
        return showMenu(chatId, messageId, mainMenu);
      }
      return ctx.answerCallbackQuery({ text: `❌ Подключение не найдено. Добавьте @${botInfo.username} в настройки.`, show_alert: true });
    }
    if (data === 'menu_ref') {
      const referralMenu = menus.getRefMenu(userId);
      await showMenu(chatId, messageId, referralMenu);
      return ctx.answerCallbackQuery();
    }
    if (data === 'menu_import_history') {
      const keyboard = new InlineKeyboard().text({ text: 'Назад', callback_data: 'back_main', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
      await editOrSend(chatId, messageId, `Для импорта истории отправьте команду <code>/archive</code>, затем загрузите JSON/ZIP.`, keyboard);
      return ctx.answerCallbackQuery();
    }
    if (data === 'menu_sub') {
      const subscriptionMenu = menus.getSubMenu(userId);
      await showMenu(chatId, messageId, subscriptionMenu);
      return ctx.answerCallbackQuery();
    }
    if (data === 'menu_premium') {
      const premiumMenu = menus.getPremiumMenu(userId);
      await showMenu(chatId, messageId, premiumMenu);
      return ctx.answerCallbackQuery();
    }
    if (data === 'buy_sub_7' || data === 'buy_sub_30' || data === 'buy_sub_90' || data === 'buy_sub_premium') {
      let title = 'Подписка на 7 дней';
      let description = 'Доступ ко всем функциям на 7 дней.';
      let payload = 'sub_7';
      let amount = Number(SUB_PRICES && SUB_PRICES.days7) || 25;
      if (data === 'buy_sub_30') {
        title = 'Подписка на 30 дней'; description = 'Доступ ко всем функциям на 30 дней.'; payload = 'sub_30'; amount = Number(SUB_PRICES && SUB_PRICES.days30) || 75;
      } else if (data === 'buy_sub_90') {
        title = 'Подписка на 90 дней'; description = 'Доступ ко всем функциям на 90 дней.'; payload = 'sub_90'; amount = Number(SUB_PRICES && SUB_PRICES.days90) || 150;
      } else if (data === 'buy_sub_premium') {
        title = 'Премиум-подписка на 1 год'; description = 'Премиум-тариф и 365 дней подписки.'; payload = 'sub_premium'; amount = Number(SUB_PRICES && SUB_PRICES.premium) || 300;
      }
      await api.sendInvoice(chatId, title, description, `${payload}_${userId}_${Date.now()}`, 'XTR', [{ label: title, amount }]);
      return ctx.answerCallbackQuery();
    }
    if (data === 'archive_import_cancel') {
      userStates.delete(userId);
      const mainMenu = menus.getMainMenu(query.from);
      await showMenu(chatId, messageId, mainMenu);
      return ctx.answerCallbackQuery({ text: 'Импорт отменён.' });
    }
    if (data === 'archive_import_again') {
      userStates.set(userId, { action: 'archive_wait_file' });
      const keyboard = new InlineKeyboard().text({ text: 'Отмена', callback_data: 'archive_import_cancel', style: 'danger' });
      await editOrSend(chatId, messageId, `Отправьте JSON или ZIP-архив экспорта Telegram Desktop.`, keyboard);
      return ctx.answerCallbackQuery();
    }
    if (data === 'menu_export_chats' || data === 'export_choose_chat' || data === 'archive_choose_chat') {
      await ctx.answerCallbackQuery({ text: 'Загружаем список чатов...' });
      return showExportChatSelection(chatId, messageId, userId);
    }
    if (data.startsWith('arch_sel_')) {
      const targetChatId = data.replace('arch_sel_', '');
      const chats = await collectAvailableChats(userId);
      const selected = chats.find((chat) => String(chat.chat_id) === String(targetChatId));
      if (!selected) return ctx.answerCallbackQuery({ text: 'Чат не найден.', show_alert: true });
      await startExportForChat(chatId, messageId, userId, selected);
      return ctx.answerCallbackQuery();
    }
    if (data.startsWith('arch_toggle_')) {
      const typeKey = normalizeArchiveType(data.replace('arch_toggle_', ''));
      const session = archiveSessions.get(userId);
      if (!session || !session.selectedChatId) return ctx.answerCallbackQuery({ text: 'Сессия истекла.', show_alert: true });
      if (session.types.has(typeKey)) { session.types.delete(typeKey); } else { session.types.add(typeKey); }
      archiveSessions.set(userId, session);
      const typesMenu = menus.getArchiveTypesMenu(session);
      await editOrSend(chatId, messageId, typesMenu.text, typesMenu.reply_markup);
      return ctx.answerCallbackQuery();
    }
    if (data === 'arch_cancel') {
      archiveSessions.delete(userId);
      const mainMenu = menus.getMainMenu(query.from);
      await showMenu(chatId, messageId, mainMenu);
      return ctx.answerCallbackQuery({ text: 'Отменено.' });
    }
    if (data === 'arch_start') {
      const session = archiveSessions.get(userId);
      if (!session || !session.selectedChatId) return ctx.answerCallbackQuery({ text: 'Выберите чат.', show_alert: true });
      if (!session.types || session.types.size === 0) return ctx.answerCallbackQuery({ text: 'Выберите типы сообщений.', show_alert: true });
      await ctx.answerCallbackQuery({ text: 'Генерация архива...' });
      await editOrSend(chatId, messageId, `<tg-emoji emoji-id="5877482652302315100">⏳</tg-emoji> <b>Генерация HTML-архива...</b>\n<tg-emoji emoji-id="5877477244938489129">👤</tg-emoji> Чат: <b>${escapeHtml(session.selectedChatName)}</b>\n\nСборка...`, new InlineKeyboard());
      try {
        const result = await generateArchiveFile(userId, session.selectedChatId, session.selectedChatName, session.types);
        const safePath = cleanFilePath(result && result.filePath ? result.filePath : result);
        if (!safePath || !fs.existsSync(safePath)) throw new Error('Ошибка generation.');
        const caption = `<tg-emoji emoji-id="5877482652302315100">📦</tg-emoji> <b>HTML-архив готов!</b>`;
        try {
          if (typeof business.safeSendHtmlDocument === 'function') {
            await business.safeSendHtmlDocument(chatId, safePath, caption);
          } else {
            await api.sendDocument(chatId, new InputFile(safePath, `chat_${session.selectedChatId}.html`), { caption, parse_mode: 'HTML' });
          }
        } finally { try { fs.unlinkSync(safePath); } catch (_) {} }
        archiveSessions.delete(userId);
        const mainMenu = menus.getMainMenu(query.from);
        return showMenu(chatId, messageId, mainMenu);
      } catch (error) {
        const typesMenu = menus.getArchiveTypesMenu(session);
        return editOrSend(chatId, messageId, `❌ Ошибка: <code>${escapeHtml(error.message)}</code>`, typesMenu.reply_markup);
      }
    }
    if (data === 'menu_admin') {
      if (!isAdmin(userId)) return ctx.answerCallbackQuery({ text: '⛔️ Нет доступа.', show_alert: true });
      adminStates.delete(userId);
      const adminMenu = menus.getAdminMenu();
      await showMenu(chatId, messageId, adminMenu);
      return ctx.answerCallbackQuery();
    }
    if (data.startsWith('admin_')) {
      if (!isAdmin(userId)) return ctx.answerCallbackQuery({ text: '⛔️ Нет доступа.', show_alert: true });
      const backAdminKeyboard = new InlineKeyboard().text({ text: 'Назад в админ-панель', callback_data: 'menu_admin', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
      if (data === 'admin_give_sub') {
        adminStates.set(userId, { action: 'give_sub' });
        await editOrSend(chatId, messageId, `➕ <b>Выдача подписки</b>\n\nОтправьте ID и количество дней через пробел:`, backAdminKeyboard);
      } else if (data === 'admin_toggle_prem') {
        adminStates.set(userId, { action: 'toggle_prem' });
        await editOrSend(chatId, messageId, `⭐️ <b>Выдать / снять Премиум</b>\n\nОтправьте Telegram ID:`, backAdminKeyboard);
      } else if (data === 'admin_ban_user') {
        adminStates.set(userId, { action: 'ban_user' });
        await editOrSend(chatId, messageId, `🔨 <b>Бан пользователя</b>\n\nОтправьте Telegram ID:`, backAdminKeyboard);
      } else if (data === 'admin_unban_user') {
        adminStates.set(userId, { action: 'unban_user' });
        await editOrSend(chatId, messageId, `🔓 <b>Разбан пользователя</b>\n\nОтправьте Telegram ID:`, backAdminKeyboard);
      } else if (data === 'admin_take_sub') {
        adminStates.set(userId, { action: 'take_sub' });
        await editOrSend(chatId, messageId, `➖ <b>Аннулирование подписки</b>\n\nОтправьте Telegram ID:`, backAdminKeyboard);
      } else if (data === 'admin_check_user') {
        adminStates.set(userId, { action: 'check_user' });
        await editOrSend(chatId, messageId, `🔍 <b>Проверка пользователя</b>\n\nОтправьте Telegram ID:`, backAdminKeyboard);
      } else if (data === 'admin_broadcast') {
        adminStates.set(userId, { action: 'broadcast' });
        await editOrSend(chatId, messageId, `📢 <b>Рассылка всем</b>\n\nОтправьте текст или медиа:`, backAdminKeyboard);
      }
      return ctx.answerCallbackQuery();
    }
    return ctx.answerCallbackQuery();
  } catch (error) {
    console.error('[callback_query Error]:', error);
    ctx.answerCallbackQuery({ text: 'Ошибка.', show_alert: true }).catch(() => {});
  }
});

bot.on('message', async (ctx) => {
  const message = ctx.message;
  try {
    if (!message.chat || message.chat.type !== 'private') return;
    if (message.text && message.text.startsWith('/')) return;
    if (!message.from || !message.from.id) return;
    const userId = message.from.id;
    const chatId = message.chat.id;
    ensureUserRecord(message.from);
    if (message.text === '❌ Отмена') {
      userStates.delete(userId);
      adminStates.delete(userId);
      await api.sendMessage(chatId, 'Действие отменено.', { reply_markup: { remove_keyboard: true } });
      const mainMenu = menus.getMainMenu(message.from);
      return menus.sendMenu(bot, chatId, mainMenu);
    }
    const userState = userStates.get(userId);
    if (userState && userState.action === 'archive_wait_file') {
      if (message.document) {
        await importArchiveDocument(message, userId);
        return;
      }
      return api.sendMessage(chatId, 'Теперь отправьте JSON или ZIP-архив экспорта Telegram Desktop.');
    }
    const adminState = adminStates.get(userId);
    if (adminState && isAdmin(userId)) {
      const backAdminKeyboard = new InlineKeyboard().text({ text: 'В админ-панель', callback_data: 'menu_admin', icon_custom_emoji_id: keys.EMOJI.back, style: 'primary' });
      if (adminState.action === 'give_sub' && message.text) {
        const parts = message.text.trim().split(/\s+/);
        const targetId = Number(parts[0]);
        const days = Number(parts[1] || 30);
        if (!Number.isInteger(targetId) || targetId <= 0 || !Number.isInteger(days) || days <= 0) {
          return sendHtml(chatId, 'Укажите ID и положительное количество дней через пробел: 123456789 30');
        }
        ensureUserRecord({ id: targetId, first_name: 'Пользователь' });
        addSubscriptionDays(targetId, days, false);
        adminStates.delete(userId);
        sendHtml(targetId, `<tg-emoji emoji-id="6028530359975548369">💎</tg-emoji> Администратор выдал вам подписку на <b>${days} дн.</b>!`).catch(() => {});
        return sendHtml(chatId, `<tg-emoji emoji-id="5776375003280838798">✅</tg-emoji> Пользователю <code>${targetId}</code> начислено <b>${days} дн.</b> подписки.`, { reply_markup: backAdminKeyboard });
      }
      if (adminState.action === 'toggle_prem' && message.text) {
        const targetId = Number(message.text.trim());
        if (!Number.isInteger(targetId) || targetId <= 0) return api.sendMessage(chatId, 'Введите корректный ID пользователя.');
        ensureUserRecord({ id: targetId, first_name: 'Пользователь' });
        const targetUser = getUser(targetId);
        const nextPremium = targetUser && Number(targetUser.is_premium) === 1 ? 0 : 1;
        if (typeof db.setPremium === 'function') {
          db.setPremium(targetId, nextPremium);
        } else if (typeof db.updateUser === 'function') {
          db.updateUser(targetId, { is_premium: nextPremium });
        } else {
          return api.sendMessage(chatId, 'В database.js отсутствует метод изменения статуса Premium.');
        }
        adminStates.delete(userId);
        return sendHtml(chatId, `<tg-emoji emoji-id="5879785854284599288">⭐️</tg-emoji> Статус Premium для <code>${targetId}</code>: <b>${nextPremium ? 'Выдан (365 дн.) ✅' : 'Снят ❌'}</b>`, { reply_markup: backAdminKeyboard });
      }
      if (adminState.action === 'ban_user' && message.text) {
        const targetId = Number(message.text.trim());
        if (!Number.isInteger(targetId) || targetId <= 0) return api.sendMessage(chatId, 'Введите корректный ID пользователя.');
        if (typeof db.setBan === 'function') {
          db.setBan(targetId, 1);
        } else if (typeof db.updateUser === 'function') {
          db.updateUser(targetId, { is_banned: 1 });
        } else {
          return api.sendMessage(chatId, 'В database.js отсутствует метод блокировки пользователя.');
        }
        adminStates.delete(userId);
        return sendHtml(chatId, `<tg-emoji emoji-id="5778527486270770928">🔨</tg-emoji> Пользователь <code>${targetId}</code> заблокирован.`, { reply_markup: backAdminKeyboard });
      }
      if (adminState.action === 'unban_user' && message.text) {
        const targetId = Number(message.text.trim());
        if (!Number.isInteger(targetId) || targetId <= 0) return api.sendMessage(chatId, 'Введите корректный ID пользователя.');
        if (typeof db.setBan === 'function') {
          db.setBan(targetId, 0);
        } else if (typeof db.updateUser === 'function') {
          db.updateUser(targetId, { is_banned: 0 });
        } else {
          return api.sendMessage(chatId, 'В database.js отсутствует метод разблокировки пользователя.');
        }
        adminStates.delete(userId);
        return sendHtml(chatId, `<tg-emoji emoji-id="5778527486270770928">🔓</tg-emoji> Пользователь <code>${targetId}</code> разблокирован.`, { reply_markup: backAdminKeyboard });
      }
      if (adminState.action === 'take_sub' && message.text) {
        const targetId = Number(message.text.trim());
        if (!Number.isInteger(targetId) || targetId <= 0) return api.sendMessage(chatId, 'Введите корректный ID пользователя.');
        if (typeof db.resetSubscription === 'function') {
          db.resetSubscription(targetId);
        } else if (typeof db.removeSubscription === 'function') {
          db.removeSubscription(targetId);
        } else if (typeof db.updateUser === 'function') {
          db.updateUser(targetId, { subscription_until: 0, sub_until: 0, is_premium: 0 });
        } else {
          return api.sendMessage(chatId, 'В database.js отсутствует метод отмены подписки.');
        }
        adminStates.delete(userId);
        return sendHtml(chatId, `<tg-emoji emoji-id="5778527486270770928">➖</tg-emoji> Подписка пользователя <code>${targetId}</code> аннулирована.`, { reply_markup: backAdminKeyboard });
      }
      if (adminState.action === 'check_user' && message.text) {
        const targetId = Number(message.text.trim());
        if (!Number.isInteger(targetId) || targetId <= 0) return api.sendMessage(chatId, 'Введите корректный ID пользователя.');
        const targetUser = getUser(targetId);
        adminStates.delete(userId);
        if (!targetUser) return sendHtml(chatId, `❌ Пользователь <code>${targetId}</code> не найден в базе данных.`, { reply_markup: backAdminKeyboard });
        const connection = typeof db.getBusinessConnectionByOwner === 'function' ? db.getBusinessConnectionByOwner(targetId) : null;
        const info = `<tg-emoji emoji-id="5877477244938489129">👤</tg-emoji> <b>Досье пользователя</b> <code>${targetId}</code>\n\n• Имя: <b>${menus.formatUserNick(targetUser)}</b>\n• Статус подписки: <b>${menus.formatSubStatus(targetUser)}</b>\n• Премиум: <b>${Number(targetUser.is_premium) ? 'Да ⭐️' : 'Нет'}</b>\n• Бан: <b>${isBanned(targetUser) ? 'Да 🚫' : 'Нет'}</b>\n• Рефералов: <b>${Number(targetUser.referrals_count) || 0}</b>\n• Бизнес-режим: <b>${connection && isEnabled(connection.is_enabled) ? 'Подключён ✅' : 'Не подключён ❌'}</b>`;
        return sendHtml(chatId, info, { reply_markup: backAdminKeyboard });
      }
      if (adminState.action === 'broadcast') {
        adminStates.delete(userId);
        const users = typeof db.getAllUsers === 'function' ? db.getAllUsers() || [] : [];
        const statusMessage = await sendHtml(chatId, `<tg-emoji emoji-id="5877465816030515018">📢</tg-emoji> Запущена рассылка по <b>${users.length}</b> пользователям...`);
        let successCount = 0;
        let failCount = 0;
        for (const targetUser of users) {
          const targetId = targetUser.user_id || targetUser.id;
          if (!targetId) continue;
          try {
            await sendBroadcastMessage(targetId, message);
            successCount++;
          } catch (_) {
            failCount++;
          }
          await config.sleep(35);
        }
        return editOrSend(chatId, statusMessage.message_id, `<tg-emoji emoji-id="5776375003280838798">✅</tg-emoji> <b>Рассылка завершена!</b>\n\n• Успешно доставлено: <b>${successCount}</b>\n• Ошибок / блоков: <b>${failCount}</b>`, backAdminKeyboard);
      }
    }
    if (message.document) {
      const fileName = String(message.document.file_name || '').toLowerCase();
      if (fileName.endsWith('.json') || fileName.endsWith('.zip')) {
        return sendHtml(chatId, 'Для импорта сначала отправьте команду /archive, затем загрузите JSON или ZIP-файл.');
      }
    }
  } catch (error) {
    console.error('[Message Error]:', error);
  }
});

process.on('unhandledRejection', (reason) => { console.error('[UnhandledRejection]:', reason); });
process.on('uncaughtException', (error) => { console.error('[UncaughtException]:', error); });

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.once(signal, async () => {
    try {
      await config.stopBot();
    } finally {
      process.exit(0);
    }
  });
}

(async () => {
  try {
    await api.setMyCommands([
      { command: 'start', description: 'Главное меню' },
      { command: 'export', description: 'Создать HTML-архив переписки' },
      { command: 'archive', description: 'Импорт истории JSON или ZIP' },
      { command: 'admin', description: 'Админ-панель' }
    ]).catch(() => {});
    await config.startBot();
  } catch (error) {
    console.error('[Init Error]:', error);
    process.exit(1);
  }
})();
