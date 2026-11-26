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

      const request = https.get(fileUrl, (response) => {
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
  } catch {
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
    const ownerId = conn.user_id;
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
  } catch (error) {
    console.error('[Business Connection Error]:', error);
  }
}

async function onBusinessMessage(ctx) {
  try {
    const message = ctx.businessMessage;
    if (!message || !message.from) return;

    if (typeof db.saveMessage === 'function') {
      db.saveMessage(message);
    }
  } catch (error) {
    console.error('[Business Message Error]:', error);
  }
}

async function onEditedBusinessMessage(ctx) {
  try {
    const message = ctx.editedBusinessMessage;
    if (!message) return;

    if (typeof db.saveEditedMessage === 'function') {
      db.saveEditedMessage(message);
    }
  } catch (error) {
    console.error('[Edited Business Message Error]:', error);
  }
}

async function onDeletedBusinessMessages(ctx) {
  try {
    const del = ctx.deletedBusinessMessages;
    if (!del || !del.message_ids) return;

    if (typeof db.markMessagesDeleted === 'function') {
      db.markMessagesDeleted(del.chat.id, del.message_ids, del.business_connection_id);
    }
  } catch (error) {
    console.error('[Deleted Business Messages Error]:', error);
  }
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

  registerBusinessHandlers
};
