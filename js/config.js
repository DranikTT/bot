'use strict';

const fs = require('fs');
const path = require('path');
const { Bot } = require('grammy');
const { run, sequentialize } = require('@grammyjs/runner');

(function loadEnv() {
  // .env ищем в двух местах: рядом с кодом (js/.env) и в корне проекта
  // (../.env). Раньше смотрели только рядом с кодом, поэтому .env,
  // положенный по инструкции в корень, молча не читался и бот падал
  // с «Не задан BOT_TOKEN».
  const candidates = [
    path.join(__dirname, '.env'),
    path.join(__dirname, '..', '.env')
  ];

  const envPath = candidates.find((candidate) => fs.existsSync(candidate));
  if (!envPath) return;

  console.log(`Конфигурация: ${envPath}`);

  for (const rawLine of fs.readFileSync(envPath, 'utf8').split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#')) continue;

    const match = line.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/);
    if (!match) continue;

    let value = match[2].trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }

    if (process.env[match[1]] === undefined) process.env[match[1]] = value;
  }
})();

function envNumber(name, fallback) {
  const value = Number(process.env[name]);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

const BOT_TOKEN = process.env.BOT_TOKEN;
if (!BOT_TOKEN) {
  throw new Error('Не задан BOT_TOKEN. Добавьте его в .env');
}

const ADMIN_IDS = String(process.env.ADMIN_IDS || '')
  .split(/[,\s]+/)
  .map(Number)
  .filter((id) => Number.isFinite(id) && id > 0);

const SUB_PRICES = { days7: 25, days30: 75, days90: 150, premium: 300 };

// DEBUG=true — подробный лог входящих обновлений и вызовов API
const DEBUG = /^(1|true|yes|on)$/i.test(String(process.env.DEBUG || '').trim());

const MAX_EXPORT_MB = envNumber('MAX_EXPORT_MB', 48);
const MAX_SINGLE_MEDIA_MB = envNumber('MAX_SINGLE_MEDIA_MB', 35);
// MAX_IMPORT_MB — устаревшее имя лимита импорта. Действует
// MAX_IMPORT_FILE_MB (по умолчанию 200). Если задан только старый ключ,
// подхватываем его, чтобы старые .env не теряли настройку.
const MAX_IMPORT_MB = envNumber('MAX_IMPORT_MB', 0);
const MAX_IMPORT_FILE_MB = envNumber(
  process.env.MAX_IMPORT_FILE_MB ? 'MAX_IMPORT_FILE_MB' : 'MAX_IMPORT_MB',
  MAX_IMPORT_MB || 200
);

const MAX_IMPORTED_MEDIA_MB = envNumber('MAX_IMPORTED_MEDIA_MB', 100);

const MEDIA_DIR = path.join(__dirname, 'media_cache');
const TEMP_DIR = path.join(__dirname, 'temp');
fs.mkdirSync(MEDIA_DIR, { recursive: true });
fs.mkdirSync(TEMP_DIR, { recursive: true });

const ALLOWED_UPDATES = [
  'message',
  'edited_message',
  'callback_query',
  'inline_query',
  'business_connection',
  'business_message',
  'edited_business_message',
  'deleted_business_messages',
  'message_reaction',
  'message_reaction_count',
  'pre_checkout_query'
];

// API_SERVER нужен для локального Bot API и офлайн-тестов.
// Без него бот всегда шёл в api.telegram.org и на тестах просто висел.
const API_BASE = process.env.API_SERVER
  ? String(process.env.API_SERVER).replace(/\/+$/, '')
  : 'https://api.telegram.org';

// ВАЖНО: grammy сам добавляет к apiRoot путь /bot<token>/<method>.
// Если передать сюда `${API_BASE}/bot`, получится /bot/bot<token>/getMe,
// и Telegram ответит 404 Not Found (именно так падал запуск).
const bot = new Bot(BOT_TOKEN, { client: { apiRoot: API_BASE } });

const botInfo = {};

// Обновления одного владельца обрабатываем строго по порядку.
//
// Без этого переписка ломается: business_connection, business_message и
// deleted_business_messages приходят пачкой и обрабатываются конкурентно.
// Владелец ещё неизвестен, удаление приходит раньше сохранения сообщения —
// и перехват молча ничего не присылает. Ключ — владелец подключения, чтобы
// все события одной переписки выстроились в очередь, а разные люди друг
// друга не блокировали.
bot.use(
  sequentialize((ctx) => {
    // Все события одной переписки должны идти строго по порядку:
    // подключение → сообщения → правки → удаления, и только потом
    // действия владельца (нажатия в меню, которые могут выключить
    // уведомления). Если ключи разные, нажатие «выключить» обгоняет
    // удаление, и бот молча не присылает перехват.
    //
    // Ключ — владелец: у business-событий его берём из подключения,
    // у нажатий в меню владельца — из ctx.from.
    const connectionId =
      ctx.businessConnectionId ||
      (ctx.businessConnection && ctx.businessConnection.id) ||
      (ctx.businessMessage && ctx.businessMessage.business_connection_id) ||
      (ctx.editedBusinessMessage && ctx.editedBusinessMessage.business_connection_id) ||
      (ctx.deletedBusinessMessages && ctx.deletedBusinessMessages.business_connection_id);

    if (connectionId) {
      // владелец подключения: из базы, иначе — из самого обновления
      const owner =
        ctx.businessConnection && ctx.businessConnection.user_chat_id;

      if (owner) return `owner:${owner}`;

      return `conn:${connectionId}`;
    }

    return `chat:${ctx.chat?.id ?? ctx.from?.id}`;
  })
);

const RETRYABLE_ERROR = /DOCUMENT_INVALID|BUTTON_ICON_INVALID|CUSTOM_EMOJI/i;
const RETRY_METHODS = new Set([
  'sendMessage',
  'editMessageText',
  'sendPhoto',
  'sendDocument',
  'sendMediaGroup'
]);

// Убирает кастомные эмодзи из текста (<tg-emoji ...>…</tg-emoji> → обычный символ)
// и из кнопок (icon_custom_emoji_id). Нужно только для аварийного повтора,
// когда Telegram отказался принимать сообщение с эмодзи.
function stripTextEmojis(payload) {
  const next = { ...payload };

  for (const field of ['text', 'caption']) {
    if (typeof next[field] !== 'string') continue;

    next[field] = next[field].replace(
      /<tg-emoji\s+emoji-id="[^"]*">([^<]*)<\/tg-emoji>/g,
      '$1'
    );
  }

  return next;
}

function stripKeyboardEmojis(markup) {
  if (!markup || !Array.isArray(markup.inline_keyboard)) return markup;

  const clone = JSON.parse(JSON.stringify(markup));

  for (const row of clone.inline_keyboard) {
    for (const button of row) {
      delete button.icon_custom_emoji_id;
    }
  }

  return clone;
}

// Кастомные эмодзи включены ВСЕГДА. Отключения нет по решению владельца:
// у его аккаунта есть Telegram Premium, поэтому право на эмодзи есть.
// Если Telegram когда-нибудь ответит DOCUMENT_INVALID, бот попробует
// отправить без эмодзи ОДИН раз, чтобы меню дошло, но режим не запоминает
// отказ и в следующий раз снова шлёт эмодзи.

bot.api.config.use(async (prev, method, payload, signal) => {
  let response = await prev(method, payload, signal);

  if (response.ok || !RETRY_METHODS.has(method)) return response;
  if (!RETRYABLE_ERROR.test(response.description || '')) return response;

  // Telegram отказал. Пробуем дослать без эмодзи, чтобы меню не потерялось,
  // но ничего не запоминаем: следующая отправка снова будет с эмодзи.
  console.warn(
    `[Custom Emoji] ${method}: ${response.description}. ` +
      'Повторяю без эмодзи, чтобы сообщение дошло.'
  );

  const variants = [
    { keyboard: true, text: false },
    { keyboard: false, text: true },
    { keyboard: true, text: true }
  ];

  for (const variant of variants) {
    let next = { ...payload };

    if (variant.keyboard && next.reply_markup) {
      next.reply_markup = stripKeyboardEmojis(next.reply_markup);
    }

    if (variant.text) next = stripTextEmojis(next);

    response = await prev(method, next, signal);

    if (response.ok || !RETRYABLE_ERROR.test(response.description || '')) {
      return response;
    }
  }

  return response;
});

bot.catch((err) => {
  const updateId = err.ctx && err.ctx.update && err.ctx.update.update_id;
  console.error(`Ошибка при обработке update ${updateId}:`, err.error);
});

function collectCustomEmojiIds() {
  const ids = new Set();

  for (const file of ['./menuKeys', './keyboards']) {
    try {
      const module_ = require(file);
      for (const value of Object.values(module_.EMOJI || {})) {
        if (/^\d+$/.test(String(value))) ids.add(String(value));
      }
    } catch {}
  }

  return Array.from(ids).slice(0, 200);
}

async function warmCustomEmojis() {
  const ids = collectCustomEmojiIds();
  if (ids.length === 0) return;

  try {
    await bot.api.getCustomEmojiStickers(ids);
  } catch (error) {
    console.warn('Не удалось прогреть custom emoji:', error.message);
  }
}

const adminStates = new Map();
const userStates = new Map();
const archiveSessions = new Map();
const deleteBatches = new Map();
const bypassProtectedMsgs = new Map();

const mutedDeletedIds = new Set();
const botGeneratedMsgIds = new Set();
const sentSelfDestructMsgIds = new Set();

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function trackSetWithLimit(set, value, limit = 5000) {
  set.add(value);

  while (set.size > limit) {
    set.delete(set.values().next().value);
  }
}

function isDotMessage(message) {
  return ['.', ','].includes(String((message && message.text) || '').trim());
}

async function downloadFileBuffer(fileId, maxBytes = Infinity) {
  const file = await bot.api.getFile(fileId);

  if (!file || !file.file_path) {
    throw new Error('Telegram не вернул путь к файлу.');
  }

  if (file.file_size && file.file_size > maxBytes) {
    throw new Error('Файл слишком большой.');
  }

  const url = `${API_BASE}/file/bot${BOT_TOKEN}/${encodeURI(file.file_path)}`;
  const response = await fetch(url);

  if (!response.ok) {
    throw new Error(`Ошибка загрузки файла: HTTP ${response.status}`);
  }

  const buffer = Buffer.from(await response.arrayBuffer());

  if (buffer.length > maxBytes) {
    throw new Error('Файл слишком большой.');
  }

  return buffer;
}

let runner = null;

async function startBot() {
  if (runner) return runner;

  await bot.init();
  Object.assign(botInfo, bot.botInfo);

  await bot.api.deleteWebhook().catch(() => {});

  runner = run(bot, {
    runner: { fetch: { allowed_updates: ALLOWED_UPDATES } }
  });

  warmCustomEmojis().catch(() => {});

  console.log(`Бот @${botInfo.username} запущен.`);
  return runner;
}

async function stopBot() {
  if (runner && runner.isRunning()) {
    await runner.stop();
  }

  runner = null;
}

module.exports = {
  bot,
  botInfo,
  DEBUG,
  API_BASE,
  BOT_TOKEN,
  ADMIN_IDS,
  SUB_PRICES,
  MAX_EXPORT_MB,
  MAX_SINGLE_MEDIA_MB,
  MAX_IMPORT_MB,
  MAX_IMPORT_FILE_MB,
  MAX_IMPORTED_MEDIA_MB,
  MEDIA_DIR,
  TEMP_DIR,
  ALLOWED_UPDATES,
  adminStates,
  userStates,
  archiveSessions,
  deleteBatches,
  bypassProtectedMsgs,
  mutedDeletedIds,
  botGeneratedMsgIds,
  sentSelfDestructMsgIds,
  sleep,
  trackSetWithLimit,
  isDotMessage,
  downloadFileBuffer,
  warmCustomEmojis,
  startBot,
  stopBot
};
