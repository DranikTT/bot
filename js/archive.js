const fs = require('fs');
const path = require('path');

function cleanPath(p) {
  if (!p) return null;
  if (typeof p === 'string') return p;
  if (typeof p === 'object') {
    if (typeof p.filePath === 'string') return p.filePath;
    if (typeof p.path === 'string') return p.path;
    if (typeof p.file_path === 'string') return p.file_path;
  }
  return null;
}

function escapeHtml(str) {
  if (str === null || str === undefined) return '';
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

function formatBytes(bytes) {
  if (!bytes || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

function formatDate(timestamp) {
  if (!timestamp) return '';
  const d = new Date(timestamp * 1000);
  return d.toLocaleString('ru-RU', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  });
}

function parseDesktopText(text) {
  if (!text) return '';
  if (typeof text === 'string') return text;
  if (Array.isArray(text)) {
    return text.map((item) => {
      if (typeof item === 'string') return item;
      if (typeof item === 'object' && item !== null && item.text) return item.text;
      return '';
    }).join('');
  }
  return String(text);
}

function buildChatHtmlArchive(chatTitle, chatId, messages) {
  let messagesListHtml = '';

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    const isOut = Boolean(msg.is_outgoing);
    const alignClass = isOut ? 'out' : 'in';
    const authorName = isOut ? 'Вы' : escapeHtml(chatTitle || `Чат ${chatId}`);
    const timeStr = formatDate(msg.date);

    let contentHtml = '';
    let isOnlySticker = false;

    if (msg.text && msg.text.trim().length > 0) {
      contentHtml += `<div class="bubble-text">${escapeHtml(msg.text).replace(/\n/g, '<br>')}</div>`;
    }

    if (msg.type === 'photo') {
      if (msg.mediaData) {
        contentHtml += `
          <div class="media-container photo">
            <img src="data:${msg.mediaMime || 'image/jpeg'};base64,${msg.mediaData}" alt="Фотография" loading="lazy" />
          </div>`;
      } else {
        contentHtml += `<div class="media-placeholder">[📷 Фотография]</div>`;
      }
    } else if (msg.type === 'sticker') {
      isOnlySticker = !msg.text;
      if (msg.mediaData) {
        if (msg.mediaMime === 'video/webm' || (msg.mediaMime && msg.mediaMime.includes('webm'))) {
          contentHtml += `
            <div class="media-container sticker video-sticker">
              <video src="data:video/webm;base64,${msg.mediaData}" autoplay loop muted playsinline></video>
            </div>`;
        } else {
          contentHtml += `
            <div class="media-container sticker">
              <img src="data:${msg.mediaMime || 'image/webp'};base64,${msg.mediaData}" alt="Стикер" loading="lazy" />
            </div>`;
        }
      } else {
        contentHtml += `<div class="media-placeholder">[🎭 Стикер]</div>`;
      }
    } else if (msg.type === 'voice') {
      if (msg.mediaData) {
        contentHtml += `
          <div class="media-container voice">
            <audio controls preload="none">
              <source src="data:${msg.mediaMime || 'audio/ogg'};base64,${msg.mediaData}" type="${msg.mediaMime || 'audio/ogg'}">
              Ваш браузер не поддерживает встроенный плеер аудио.
            </audio>
          </div>`;
      } else {
        contentHtml += `<div class="media-placeholder">[🎤 Голосовое сообщение]</div>`;
      }
    } else if (msg.type === 'video_note') {
      if (msg.mediaData) {
        contentHtml += `
          <div class="media-container round-video">
            <video src="data:${msg.mediaMime || 'video/mp4'};base64,${msg.mediaData}" controls playsinline preload="metadata"></video>
          </div>`;
      } else {
        contentHtml += `<div class="media-placeholder">[🎥 Видеосообщение (кружок)]</div>`;
      }
    } else if (msg.type === 'video') {
      if (msg.mediaData) {
        contentHtml += `
          <div class="media-container standard-video">
            <video src="data:${msg.mediaMime || 'video/mp4'};base64,${msg.mediaData}" controls playsinline preload="metadata"></video>
          </div>`;
      } else {
        contentHtml += `<div class="media-placeholder">[🎬 Видеозапись]</div>`;
      }
    } else if (msg.type === 'document') {
      const fileName = escapeHtml(msg.fileName || 'Документ');
      contentHtml += `
        <div class="file-attachment">
          <div class="file-icon">📁</div>
          <div class="file-info">
            <div class="file-name">${fileName}</div>
            <div class="file-sub">Вложенный файл</div>
          </div>
        </div>`;
    } else if (msg.type === 'location') {
      contentHtml += `<div class="media-placeholder">[📍 Геолокация]</div>`;
    } else if (msg.type === 'animation') {
      if (msg.mediaData) {
        contentHtml += `
          <div class="media-container photo">
            <video src="data:${msg.mediaMime || 'video/mp4'};base64,${msg.mediaData}" autoplay loop muted playsinline></video>
          </div>`;
      } else {
        contentHtml += `<div class="media-placeholder">[🎞 GIF-анимация]</div>`;
      }
    }

    const bubbleExtraClass = isOnlySticker ? 'sticker-only' : '';

    messagesListHtml += `
      <div class="message-row ${alignClass}" id="msg-${msg.id}">
        <div class="bubble ${bubbleExtraClass}">
          <div class="bubble-header">
            <span class="bubble-author">${authorName}</span>
            <span class="bubble-time">${timeStr}</span>
          </div>
          ${contentHtml}
        </div>
      </div>
    `;
  }

  return `<!DOCTYPE html>
<html lang="ru">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>Архив: ${escapeHtml(chatTitle)}</title>
  <style>
    :root {
      --bg-color: #0e1621;
      --chat-bg: #17212b;
      --bubble-in: #182533;
      --bubble-out: #2b5278;
      --text-main: #f5f5f5;
      --text-sub: #7f91a4;
      --accent: #5288c1;
      --border-color: #242f3d;
      --bubble-radius: 14px;
    }
    * {
      box-sizing: border-box;
      margin: 0;
      padding: 0;
      -webkit-tap-highlight-color: transparent;
    }
    body {
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      background-color: var(--bg-color);
      color: var(--text-main);
      display: flex;
      justify-content: center;
      min-height: 100vh;
      padding: 12px;
    }
    .archive-container {
      width: 100%;
      max-width: 860px;
      background: var(--chat-bg);
      border-radius: 12px;
      overflow: hidden;
      display: flex;
      flex-direction: column;
      box-shadow: 0 8px 30px rgba(0, 0, 0, 0.5);
    }
    .chat-header {
      padding: 16px 20px;
      background: #17212b;
      border-bottom: 1px solid var(--border-color);
      display: flex;
      justify-content: space-between;
      align-items: center;
      position: sticky;
      top: 0;
      z-index: 100;
    }
    .chat-title {
      font-size: 1.15rem;
      font-weight: 600;
      color: #ffffff;
    }
    .chat-info {
      font-size: 0.85rem;
      color: var(--text-sub);
      margin-top: 2px;
    }
    .chat-stats {
      text-align: right;
      font-size: 0.85rem;
      color: var(--accent);
      font-weight: 500;
    }
    .messages-list {
      padding: 20px 16px;
      display: flex;
      flex-direction: column;
      gap: 12px;
      flex: 1;
    }
    .message-row {
      display: flex;
      width: 100%;
    }
    .message-row.in {
      justify-content: flex-start;
    }
    .message-row.out {
      justify-content: flex-end;
    }
    .bubble {
      max-width: 78%;
      padding: 10px 14px;
      border-radius: var(--bubble-radius);
      position: relative;
      word-wrap: break-word;
      font-size: 0.95rem;
      line-height: 1.45;
      box-shadow: 0 1px 2px rgba(0,0,0,0.2);
    }
    .bubble.sticker-only {
      background: transparent !important;
      box-shadow: none !important;
      padding: 2px !important;
    }
    .message-row.in .bubble:not(.sticker-only) {
      background: var(--bubble-in);
      border-bottom-left-radius: 4px;
    }
    .message-row.out .bubble:not(.sticker-only) {
      background: var(--bubble-out);
      border-bottom-right-radius: 4px;
    }
    .bubble-header {
      display: flex;
      justify-content: space-between;
      align-items: baseline;
      gap: 14px;
      font-size: 0.75rem;
      color: var(--text-sub);
      margin-bottom: 4px;
    }
    .bubble-author {
      font-weight: 600;
      color: var(--accent);
    }
    .message-row.out .bubble-author {
      color: #92c5ff;
    }
    .bubble-text {
      margin-top: 2px;
      word-break: break-word;
    }
    .media-container {
      margin-top: 8px;
      border-radius: 8px;
      overflow: hidden;
    }
    .media-container.photo img {
      max-width: 100%;
      max-height: 480px;
      border-radius: 8px;
      display: block;
      object-fit: cover;
    }
    .media-container.standard-video video {
      max-width: 100%;
      max-height: 480px;
      border-radius: 8px;
      display: block;
    }
    .media-container.sticker {
      background: transparent !important;
      padding: 0;
      width: 170px;
      height: 170px;
    }
    .media-container.sticker img,
    .media-container.sticker video {
      width: 100%;
      height: 100%;
      object-fit: contain;
      display: block;
    }
    .media-container.round-video {
      display: flex;
      justify-content: center;
      align-items: center;
      padding: 4px;
    }
    .media-container.round-video video {
      width: 230px;
      height: 230px;
      border-radius: 50%;
      object-fit: cover;
      display: block;
      border: 2px solid rgba(255, 255, 255, 0.1);
    }
    .media-container.voice audio {
      width: 100%;
      min-width: 250px;
      height: 40px;
      margin-top: 6px;
      outline: none;
    }
    .file-attachment {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 8px 10px;
      background: rgba(0, 0, 0, 0.15);
      border-radius: 8px;
      margin-top: 6px;
    }
    .file-icon {
      font-size: 1.6rem;
    }
    .file-info {
      display: flex;
      flex-direction: column;
      overflow: hidden;
    }
    .file-name {
      font-weight: 500;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .file-sub {
      font-size: 0.75rem;
      color: var(--text-sub);
    }
    .media-placeholder {
      margin-top: 6px;
      font-style: italic;
      color: var(--text-sub);
      font-size: 0.85rem;
    }
    @media (max-width: 650px) {
      body {
        padding: 0;
      }
      .archive-container {
        border-radius: 0;
        min-height: 100vh;
      }
      .bubble {
        max-width: 88%;
      }
      .media-container.round-video video {
        width: 190px;
        height: 190px;
      }
      .media-container.voice audio {
        min-width: 210px;
      }
    }
  </style>
</head>
<body>
  <div class="archive-container">
    <div class="chat-header">
      <div>
        <div class="chat-title">${escapeHtml(chatTitle)}</div>
        <div class="chat-info">Идентификатор чата: ${escapeHtml(chatId)}</div>
      </div>
      <div class="chat-stats">
        <div>Всего сообщений: ${messages.length}</div>
        <div style="font-size: 0.75rem; color: var(--text-sub); margin-top: 2px;">Экспорт завершен</div>
      </div>
    </div>
    <div class="messages-list">
      ${messagesListHtml}
    </div>
  </div>
</body>
</html>`;
}

function importTelegramDesktopExport(jsonFilePath, dbInstance, ownerId) {
  const targetPath = cleanPath(jsonFilePath);
  if (!targetPath || !fs.existsSync(targetPath)) {
    throw new Error('Файл импорта не найден на диске сервера.');
  }

  const raw = fs.readFileSync(targetPath, 'utf8');
  const parsed = JSON.parse(raw);

  if (!parsed || !Array.isArray(parsed.messages)) {
    throw new Error('Некорректная структура result.json: отсутствует массив messages.');
  }

  const targetChatId = parsed.id ? String(parsed.id) : `imported_${Date.now()}`;
  let count = 0;

  for (let i = 0; i < parsed.messages.length; i++) {
    const m = parsed.messages[i];
    if (m.type !== 'message') continue;

    const senderId = m.from_id ? String(m.from_id) : '0';
    const text = parseDesktopText(m.text);
    const dateTimestamp = m.date_unixtime ? Number(m.date_unixtime) : Math.floor(new Date(m.date).getTime() / 1000);

    let msgType = 'text';
    if (m.photo) msgType = 'photo';
    else if (m.media_type === 'sticker') msgType = 'sticker';
    else if (m.media_type === 'voice_message') msgType = 'voice';
    else if (m.media_type === 'round_video') msgType = 'video_note';
    else if (m.media_type === 'video_file') msgType = 'video';
    else if (m.media_type === 'animation') msgType = 'animation';
    else if (m.file) msgType = 'document';

    dbInstance.saveMessage({
      message_id: m.id,
      chat_id: targetChatId,
      owner_id: String(ownerId),
      sender_id: senderId,
      is_outgoing: senderId === String(ownerId) ? 1 : 0,
      text: text,
      media_type: msgType,
      media_file_id: null,
      file_path: m.file || m.photo || null,
      caption: null,
      ttl_seconds: 0,
      created_at: dateTimestamp || Math.floor(Date.now() / 1000)
    });

    count++;
  }

  return count;
}

module.exports = {
  cleanPath,
  escapeHtml,
  formatBytes,
  formatDate,
  parseDesktopText,
  buildChatHtmlArchive,
  importTelegramDesktopExport
};
