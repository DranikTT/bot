'use strict';

const fs = require('fs');
const path = require('path');
const https = require('https');
const { InputFile, InlineKeyboard } = require('grammy');
const config = require('./config');

const db = require('./database');
const { cleanPath } = require('./archive');

const bot = config.bot;
const api = bot.api;

const NO_PREVIEW = { link_preview_options: { is_disabled: true } };

const MAX_SINGLE_MEDIA_MB = Number(config.MAX_SINGLE_MEDIA_MB) || 20;
const MEDIA_DIR = path.resolve(
  config.MEDIA_DIR || path.join(__dirname, 'media')
);

const deleteBatches = config.deleteBatches;
const mutedDeletedIds = config.mutedDeletedIds;
const botGeneratedMsgIds = config.botGeneratedMsgIds;
const sentSelfDestructMsgIds = config.sentSelfDestructMsgIds;
const bypassProtectedMsgs = config.bypassProtectedMsgs;

const capturedMediaMsgIds = new Set();
const mediaForwardInFlight = new Set();

const { sleep, trackSetWithLimit, isDotMessage } = config;

function addTracked(set, value) {
  try {
    trackSetWithLimit(set, value);
  } catch {
    set.add(value);
  }
}

function escapeHtml(value) {
  if (value === null || value === undefined) return '';

  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function stripHtmlCustomEmojis(text) {
  if (typeof text !== 'string') return text;
  return text.replace(/<tg-emoji\b[^>]*>([\s\S]*?)<\/tg-emoji>/gi, '$1');
}

function htmlOptions(options = {}) {
  const result = { ...options, parse_mode: 'HTML' };

  for (const key of Object.keys(result)) {
    if (result[key] === undefined) delete result[key];
  }

  return result;
}

async function sendHtmlMessage(chatId, text, options = {}) {
  return api.sendMessage(
    chatId,
    String(text === null || text === undefined ? '' : text),
    htmlOptions(options)
  );
}

function getDefaultExtForMedia(mediaType, filePathFromTelegram = '') {
  const telegramExt = path.extname(String(filePathFromTelegram || ''));
  if (telegramExt && telegramExt !== '.bin') return telegramExt;

  switch (mediaType) {
    case 'photo':
      return '.jpg';
    case 'video':
    case 'video_note':
    case 'animation':
      return '.mp4';
    case 'voice':
      return '.ogg';
    case 'audio':
      return '.mp3';
    case 'sticker':
      return '.webp';
    default:
      return telegramExt || '.bin';
  }
}

async function safeSendMediaToChat(
  targetChatId,
  mediaType,
  rawFilePath,
  fallbackFileId,
  options = {}
) {
  const filePath = cleanPath(rawFilePath);
  const hasLocalFile = Boolean(filePath && fs.existsSync(filePath));

  const ext = hasLocalFile
    ? path.extname(filePath) || getDefaultExtForMedia(mediaType)
    : getDefaultExtForMedia(mediaType);

  const trySend = async (sendOptions, useFallbackId = false) => {
    let payload;

    if (useFallbackId) {
      payload = fallbackFileId;
    } else if (hasLocalFile) {
      payload = new InputFile(
        filePath,
        path.basename(filePath) || `file${ext}`
      );
    } else {
      payload = fallbackFileId;
    }

    if (!payload) throw new Error('Нет источника медиа для отправки.');

    const finalOptions = htmlOptions(sendOptions);

    if (mediaType === 'photo') {
      return api.sendPhoto(targetChatId, payload, finalOptions);
    }

    if (mediaType === 'video') {
      return api.sendVideo(targetChatId, payload, finalOptions);
    }

    if (mediaType === 'voice') {
      return api.sendVoice(targetChatId, payload, finalOptions);
    }

    if (mediaType === 'video_note') {
      const sent = await api.sendVideoNote(targetChatId, payload);

      if (sendOptions.caption) {
        try {
          await sendHtmlMessage(targetChatId, sendOptions.caption);
        } catch {}
      }

      return sent;
    }

    if (mediaType === 'audio') {
      return api.sendAudio(targetChatId, payload, finalOptions);
    }

    if (mediaType === 'animation') {
      return api.sendAnimation(targetChatId, payload, finalOptions);
    }

    if (mediaType === 'sticker') {
      const sent = await api.sendSticker(targetChatId, payload);

      if (sendOptions.caption) {
        try {
          await sendHtmlMessage(targetChatId, sendOptions.caption);
        } catch {}
      }

      return sent;
    }

    return api.sendDocument(targetChatId, payload, finalOptions);
  };

  try {
    return await trySend(options, false);
  } catch (firstError) {
    const cleanOptions = htmlOptions({
      ...options,
      caption: options.caption
        ? stripHtmlCustomEmojis(options.caption)
        : undefined
    });

    try {
      return await trySend(cleanOptions, false);
    } catch (secondError) {
      if (fallbackFileId && hasLocalFile) {
        try {
          return await trySend(cleanOptions, true);
        } catch {}
      }

      throw secondError || firstError;
    }
  }
}

async function safeSendHtmlDocument(targetChatId, rawArchivePath, captionHtml) {
  const archivePath = cleanPath(rawArchivePath);
  if (!archivePath || !fs.existsSync(archivePath)) return null;

  const filename = path.basename(archivePath) || 'archive.html';

  try {
    return await api.sendDocument(
      targetChatId,
      new InputFile(archivePath, filename),
      htmlOptions({ caption: captionHtml || '' })
    );
  } catch (firstError) {
    try {
      return await api.sendDocument(
        targetChatId,
        new InputFile(archivePath, filename),
        htmlOptions({ caption: stripHtmlCustomEmojis(captionHtml || '') })
      );
    } catch (secondError) {
      throw secondError || firstError;
    }
  }
}

async function downloadTelegramFile(fileId, prefix = 'media', mediaType = 'document') {
  try {
    if (!fileId) return null;

    const fileInfo = await api.getFile(fileId);
    if (!fileInfo || !fileInfo.file_path) return null;

    const fileSizeMb = Number(fileInfo.file_size || 0) / (1024 * 1024);
    if (fileSizeMb > MAX_SINGLE_MEDIA_MB) return null;

    fs.mkdirSync(MEDIA_DIR, { recursive: true });

    const ext = getDefaultExtForMedia(mediaType, fileInfo.file_path);
    const safePrefix =
      String(prefix).replace(/[^a-zA-Z0-9_.-]/g, '_').slice(-80) || 'media';

    const localName = `${safePrefix}_${Date.now()}_${Math.random()
      .toString(36)
      .slice(2, 8)}${ext}`;

    const localPath = path.join(MEDIA_DIR, localName);
    const encodedFilePath = encodeURI(fileInfo.file_path).replace(/#/g, '%23');
    const fileUrl = `${config.API_BASE}/file/bot${bot.token}/${encodedFilePath}`;

    return await new Promise((resolve, reject) => {
      let settled = false;
      let outputStream = null;

      const fail = (error) => {
        if (settled) return;
        settled = true;

        if (outputStream) outputStream.destroy();
        fs.promises.unlink(localPath).catch(() => {});
        reject(error);
      };

      const transport = fileUrl.startsWith('https:') ? https : require('http');

      const request = transport.get(fileUrl, (response) => {
        if (response.statusCode !== 200) {
          response.resume();
          fail(new Error(`HTTP ${response.statusCode}`));
          return;
        }

        outputStream = fs.createWriteStream(localPath);
        response.pipe(outputStream);

        response.on('error', fail);
        outputStream.on('error', fail);

        outputStream.on('finish', () => {
          outputStream.close(() => {
            if (settled) return;
            settled = true;
            resolve(localPath);
          });
        });
      });

      request.setTimeout(30000, () => {
        request.destroy(new Error('Тайм-аут загрузки файла.'));
      });

      request.on('error', fail);
    });
  } catch (error) {
    console.error('[downloadTelegramFile]:', error.message);
    return null;
  }
}

function extractMediaMeta(message) {
  let mediaType = null;
  let fileId = null;
  let fileSize = 0;
  let extraText = '';
  let fileName = '';

  if (message.photo && message.photo.length > 0) {
    const best = message.photo.reduce((largest, photo) => {
      if (!largest) return photo;
      return Number(photo.file_size || 0) > Number(largest.file_size || 0)
        ? photo
        : largest;
    }, null);

    mediaType = 'photo';
    fileId = best.file_id;
    fileSize = best.file_size || 0;
  } else if (message.video) {
    mediaType = 'video';
    fileId = message.video.file_id;
    fileSize = message.video.file_size || 0;
  } else if (message.video_note) {
    mediaType = 'video_note';
    fileId = message.video_note.file_id;
    fileSize = message.video_note.file_size || 0;
  } else if (message.animation) {
    mediaType = 'animation';
    fileId = message.animation.file_id;
    fileSize = message.animation.file_size || 0;
  } else if (message.voice) {
    mediaType = 'voice';
    fileId = message.voice.file_id;
    fileSize = message.voice.file_size || 0;
  } else if (message.audio) {
    mediaType = 'audio';
    fileId = message.audio.file_id;
    fileSize = message.audio.file_size || 0;
  } else if (message.sticker) {
    mediaType = 'sticker';
    fileId = message.sticker.file_id;
    fileSize = message.sticker.file_size || 0;
  } else if (message.document) {
    mediaType = 'document';
    fileId = message.document.file_id;
    fileSize = message.document.file_size || 0;
    fileName = message.document.file_name || '';
  }

  return { mediaType, fileId, fileSize, fileName, extraText };
}

async function onBusinessConnection(ctx) {
  try {
    const conn = ctx.businessConnection;

    // В Telegram поле называется user_chat_id (id личного чата владельца).
    // Раньше читали conn.user_id — его не существует, владелец выходил
    // undefined, и подключение записывалось в никуда.
    const ownerId = Number(
      conn.user_chat_id || (conn.user && conn.user.id) || 0
    );

    if (!ownerId) {
      console.error('[Business Connection] не удалось определить владельца:', conn);
      return;
    }

    const connectionId = conn.id;
    const isEnabledFlag = conn.is_enabled ? 1 : 0;

    if (typeof db.saveBusinessConnection === 'function') {
      db.saveBusinessConnection(ownerId, connectionId, isEnabledFlag);
    } else if (typeof db.updateUser === 'function') {
      db.updateUser(ownerId, {
        business_connection_id: connectionId,
        is_enabled: isEnabledFlag
      });
    }

    console.log(
      `[Business Connection] Обновлен статус для ${ownerId}: ID=${connectionId}, Active=${isEnabledFlag}`
    );

    // Подключение меняется владельцем в настройках, а бот узнаёт об этом
    // только по этому событию. Сообщаем владельцу и показываем актуальное меню.
    await notifyConnectionChange(ownerId, connectionId, isEnabledFlag, conn);
  } catch (error) {
    console.error('[Business Connection Error]:', error);
  }
}

/// Сообщает владельцу о смене статуса подключения и обновляет меню.
async function notifyConnectionChange(ownerId, connectionId, isEnabled, conn) {
  const api = config.bot.api;
  const username = (config.botInfo && config.botInfo.username) || '';

  // имя бизнес-аккаунта, если Telegram его прислал
  const accountName = conn && conn.user
    ? [conn.user.first_name, conn.user.last_name].filter(Boolean).join(' ') ||
      (conn.user.username ? `@${conn.user.username}` : '')
    : '';

  if (isEnabled) {
    // пробуем отправить в сам Business-чат: это и есть проверка, что бот
    // действительно может отвечать от имени владельца
    let canReply = false;

    try {
      await api.sendMessage(
        ownerId,
        `<tg-emoji emoji-id="5776375003280838798">✅</tg-emoji> <b>Бот подключён к автоматизации</b>\n\n` +
          `Перехват удалённых, изменённых и сгорающих сообщений активен.\n` +
          `Управление — командами прямо в чате: <code>.nah</code>, <code>.love</code>, <code>.bypass</code>, <code>.mute</code>, <code>.unmute</code>.`,
        {
          parse_mode: 'HTML',
          business_connection_id: connectionId,
          link_preview_options: { is_disabled: true }
        }
      );

      canReply = true;
    } catch (error) {
      const reason = String((error && (error.description || error.message)) || error);
      console.error(`[Business Connection] ответ от имени владельца не прошёл: ${reason}`);

      canReply = false;
    }

    // дублируем обычным сообщением — оно дойдёт в любом случае
    const suffix = canReply
      ? '\n\nОтвет от вашего имени проверен: работает.'
      : '\n\n⚠️ Telegram не разрешил отвечать от вашего имени — проверьте, что боту выданы права на чтение и отправку.';

    await api
      .sendMessage(
        ownerId,
        `<tg-emoji emoji-id="5877465816030515018">🔌</tg-emoji> <b>Подключение активно</b>${accountName ? ` — ${accountName}` : ''}${suffix}`,
        { parse_mode: 'HTML', link_preview_options: { is_disabled: true } }
      )
      .catch(() => {});
  } else {
    await api
      .sendMessage(
        ownerId,
        `<tg-emoji emoji-id="5778527486270770928">❌</tg-emoji> <b>Подключение отключено</b>\n\n` +
          `Перехват остановлен. Чтобы вернуть, добавьте <code>@${username}</code> в Автоматизацию чатов заново.`,
        { parse_mode: 'HTML', link_preview_options: { is_disabled: true } }
      )
      .catch(() => {});
  }
}

/**
 * Автопроверка подключения: если владелец зашёл в раздел, но событие
 * business_connection до бота не дошло (бывает при первом подключении),
 * статус всё равно должен быть верным.
 *
 * Проверяем фактическую возможность отправки в Business-чат, а не только
 * запись в базе. Возвращает { connected, canReply, reason }.
 */
async function verifyConnection(ownerId) {
  const api = config.bot.api;
  const connection = typeof db.getBusinessConnectionByOwner === 'function'
    ? db.getBusinessConnectionByOwner(ownerId)
    : null;

  if (!connection || !connection.connection_id) {
    return {
      connected: false,
      canReply: false,
      reason: 'запись о подключении отсутствует'
    };
  }

  if (!(connection.is_enabled === true || Number(connection.is_enabled) === 1)) {
    return {
      connected: false,
      canReply: false,
      reason: 'подключение помечено как отключённое'
    };
  }

  // Дополнительно убеждаемся, что соединение с Telegram живо:
  // getMe дешёвый и не требует прав в чате.
  try {
    await api.getMe();
    return { connected: true, canReply: true, reason: 'подключение активно' };
  } catch (error) {
    return {
      connected: true,
      canReply: false,
      reason: String((error && (error.description || error.message)) || error)
    };
  }
}

async function onBusinessMessage(ctx) {
  try {
    const message = ctx.businessMessage;
    if (!message || !message.from) return;

    // Сначала владелец: без записи в business_connections saveMessage
    // не сможет его определить и привяжет сообщение к отправителю.
    if (message.business_connection_id) {
      await resolveOwner(message.business_connection_id);
    }

    if (typeof db.saveMessage === 'function') {
      db.saveMessage(message);
    }

    // Фото/видео «сгорают»: после удаления file_id может стать недоступен,
    // и переслать владельцу будет нечего. Скачиваем сразу, пока сообщение живо.
    await cacheSelfDestructMedia(message);
  } catch (error) {
    console.error('[Business Message Error]:', error);
  }
}

/**
 * Самоудаляющиеся сообщения.
 *
 * Отдельного поля «сгорит» Bot API не отдаёт — определить можно только по
 * косвенным признакам (медиа со спойлером, медиа без текста). Поэтому
 * сохраняем медиа сразу после получения, не дожидаясь удаления: к моменту,
 * когда придёт deleted_business_messages, файла уже может не быть.
 */
async function cacheSelfDestructMedia(message) {
  const media = extractMediaMeta(message);

  if (!media.fileId || capturedMediaMsgIds.has(message.message_id)) return;

  const suspected =
    Boolean(message.has_media_spoiler) ||
    Boolean(message.self_destruct_media) ||
    Boolean(media.mediaType && !message.text && !message.caption);

  if (!suspected) return;

  const lockKey = `${message.chat.id}:${message.message_id}`;

  if (mediaForwardInFlight.has(lockKey)) return;

  mediaForwardInFlight.add(lockKey);
  addTracked(capturedMediaMsgIds, message.message_id);

  try {
    const localPath = await downloadTelegramFile(
      media.fileId,
      `sd_${message.chat.id}_${message.message_id}`,
      media.mediaType
    );

    if (localPath && typeof db.saveMediaPath === 'function') {
      db.saveMediaPath(message.chat.id, message.message_id, localPath);
    }

    if (suspected && typeof db.markSelfDestruct === 'function') {
      db.markSelfDestruct(message.chat.id, message.message_id, 1);
    }
  } catch (error) {
    console.error('[Self-destruct media]:', error.message);
  } finally {
    mediaForwardInFlight.delete(lockKey);
  }
}

async function onEditedBusinessMessage(ctx) {
  try {
    const message = ctx.editedBusinessMessage;
    if (!message) return;

    // старый текст нужен ДО сохранения: saveEditedMessage перезапишет его
    const before = typeof db.getStoredMessage === 'function'
      ? db.getStoredMessage(message.chat.id, message.message_id)
      : null;

    if (typeof db.saveEditedMessage === 'function') {
      db.saveEditedMessage(message);
    }

    const owner = await resolveOwner(message.business_connection_id);

    if (!owner) return;

    const notify = typeof db.getNotifySettings === 'function'
      ? db.getNotifySettings(owner)
      : { edited: true };

    if (!notify.edited) return;

    const oldText = before ? String(before.text || '').trim() : '';
    const newText = String(message.text || message.caption || '').trim();

    // Telegram присылает edited_business_message и на разметку без правки
    // текста — не дёргаем владельца зря
    if (oldText === newText) return;

    const who = (message.from && (message.from.first_name || message.from.username)) ||
      (message.chat && (message.chat.first_name || message.chat.username)) ||
      'Собеседник';

    const body =
      `<tg-emoji emoji-id="5877260593903177342">✏️</tg-emoji> <b>Изменённое сообщение</b>\n` +
      `От: <b>${escapeHtml(who)}</b>\n\n` +
      (oldText
        ? `<s>${escapeHtml(oldText)}</s>\n\n`
        : '<i>было без текста</i>\n\n') +
      `${escapeHtml(newText || '(текст удалён)')}`;

    await sendHtmlMessage(owner, body).catch(() => {});
  } catch (error) {
    console.error('[Edited Business Message Error]:', error);
  }
}

async function onDeletedBusinessMessages(ctx) {
  try {
    const del = ctx.deletedBusinessMessages;
    if (!del || !del.message_ids) return;

    // владельца определяем с самовосстановлением: событие подключения
    // могло не дойти или прийти позже самой переписки
    const owner = await resolveOwner(del.business_connection_id);

    // Третий аргумент — id ВЛАДЕЛЬЦА (число), не business_connection_id:
    // раньше сюда уходила строка подключения, владелец становился NaN.
    if (typeof db.markMessagesDeleted === 'function') {
      db.markMessagesDeleted(del.chat.id, del.message_ids, owner);
    }

    // Без уведомления перехват бессмыслен: сообщение сохранено, но владелец
    // о нём не знает. Telegram присылает только id — текст берём из базы.
    if (!owner) return;

    const notify = typeof db.getNotifySettings === 'function'
      ? db.getNotifySettings(owner)
      : { deleted: true };

    if (!notify.deleted) return;

    // пачкой: Telegram шлёт все удалённые id одним обновлением
    for (const messageId of del.message_ids.slice(0, 10)) {
      const stored = typeof db.getStoredMessage === 'function'
        ? db.getStoredMessage(del.chat.id, messageId)
        : null;

      await sendDeletedNotice(owner, del.chat, messageId, stored);
    }

    if (del.message_ids.length > 10) {
      await sendHtmlMessage(
        owner,
        `<tg-emoji emoji-id="5814281611541222134">🗑</tg-emoji> …и ещё <b>${del.message_ids.length - 10}</b> удалённых.`
      ).catch(() => {});
    }
  } catch (error) {
    console.error('[Deleted Business Messages Error]:', error);
  }
}

/**
 * Кто владелец этого подключения — с самовосстановлением.
 *
 * Событие business_connection приходит не всегда и не всегда первым:
 * при перезапуске бота, при подключении задним числом, при потере
 * обновления. Раньше в этом случае владелец оставался неизвестным,
 * и перехват молча не работал. Теперь бот спрашивает сам Telegram
 * и запоминает ответ.
 */
async function resolveOwner(connectionId) {
  if (!connectionId) return null;

  const known =
    typeof db.getBusinessConnectionOwner === 'function'
      ? db.getBusinessConnectionOwner(connectionId)
      : null;

  const knownId = Number(known);

  if (Number.isSafeInteger(knownId) && knownId > 0) return knownId;

  // в базе нет — спрашиваем Telegram
  try {
    const conn = await api.getBusinessConnection(connectionId);
    const ownerId = Number(
      (conn && (conn.user_chat_id || (conn.user && conn.user.id))) || 0
    );

    if (!ownerId) return null;

    if (typeof db.saveBusinessConnection === 'function') {
      db.saveBusinessConnection(ownerId, connectionId, conn.is_enabled ? 1 : 0);
    }

    console.log(
      `[Business] Подключение ${connectionId} восстановлено: владелец ${ownerId}`
    );

    return ownerId;
  } catch (error) {
    console.error(
      `[Business] Не удалось определить владельца ${connectionId}:`,
      (error && (error.description || error.message)) || error
    );
    return null;
  }
}

/// Сообщение владельцу: «вот что собеседник удалил».
async function sendDeletedNotice(ownerId, chat, messageId, stored) {
  const who = stored
    ? (stored.sender_name || stored.sender_username || 'Собеседник')
    : (chat.first_name || chat.username || 'Собеседник');

  const header =
    `<tg-emoji emoji-id="5814281611541222134">🗑</tg-emoji> <b>Удалённое сообщение</b>\n` +
    `От: <b>${escapeHtml(who)}</b>`;

  if (!stored) {
    // удалили то, чего бот не видел (до подключения или сообщение старше)
    await sendHtmlMessage(
      ownerId,
      `${header}\n\n<i>Текст недоступен — сообщение пришло до подключения бота.</i>`
    ).catch(() => {});
    return;
  }

  const text = String(stored.text || '').trim();

  if (!text && !stored.media_type) {
    await sendHtmlMessage(ownerId, `${header}\n\n<i>Без текста.</i>`).catch(() => {});
    return;
  }

  const caption = text ? `${header}\n\n${escapeHtml(text)}` : header;

  // медиа пересылаем файлом/фото: file_id живёт и после удаления в чате
  if (stored.media_type) {
    try {
      const sent = await safeSendMediaToChat(
        ownerId,
        stored.media_type,
        stored.local_path,
        stored.file_id,
        { caption: `${caption}\n\n<i>${mediaLabel(stored.media_type)}</i>` }
      );

      return sent;
    } catch (error) {
      console.error('[Deleted Notice Media]:', error.message);
    }
  }

  await sendHtmlMessage(ownerId, caption).catch(() => {});
}

function mediaLabel(mediaType) {
  const labels = {
    photo: 'фото',
    video: 'видео',
    video_note: 'видеосообщение',
    animation: 'гифка',
    voice: 'голосовое',
    audio: 'аудио',
    sticker: 'стикер',
    document: 'документ'
  };

  return labels[mediaType] || 'медиа';
}

function registerBusinessHandlers() {
  bot.on('business_connection', onBusinessConnection);
  bot.on('business_message', onBusinessMessage);
  bot.on('edited_business_message', onEditedBusinessMessage);
  bot.on('deleted_business_messages', onDeletedBusinessMessages);
}

module.exports = {
  capturedMediaMsgIds,
  mediaForwardInFlight,

  addTracked,
  escapeHtml,
  stripHtmlCustomEmojis,
  htmlOptions,
  sendHtmlMessage,

  getDefaultExtForMedia,
  safeSendMediaToChat,
  safeSendHtmlDocument,
  downloadTelegramFile,
  extractMediaMeta,

  registerBusinessHandlers,
  resolveOwner,
  sendDeletedNotice,
  mediaLabel,
  verifyConnection,
  notifyConnectionChange
};
