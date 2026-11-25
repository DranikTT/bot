'use strict';

const fs = require('fs');
const path = require('path');
const { Bot } = require('grammy');
const { run, sequentialize } = require('@grammyjs/runner');
const { assertKeyboard } = require('./keyboardGuard');

(function loadEnv() {
  const envPath = path.join(__dirname, '.env');
  if (!fs.existsSync(envPath)) return;

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

const MAX_EXPORT_MB = envNumber('MAX_EXPORT_MB', 48);
const MAX_SINGLE_MEDIA_MB = envNumber('MAX_SINGLE_MEDIA_MB', 35);
const MAX_IMPORT_MB = envNumber('MAX_IMPORT_MB', 50);
const MAX_IMPORT_FILE_MB = envNumber('MAX_IMPORT_FILE_MB', 200);
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

const bot = new Bot(BOT_TOKEN);

const botInfo = {};

bot.use(sequentialize((ctx) => String(ctx.chat?.id ?? ctx.from?.id)));

const RETRYABLE_ERROR = /DOCUMENT_INVALID|BUTTON_ICON_INVALID|CUSTOM_EMOJI/i;
const RETRY_METHODS = new Set(['sendMessage', 'editMessageText']);

function stripKeyboardEmojis(markup) {
  let value = markup;

  if (typeof value === 'string') {
    try {
      value = JSON.parse(value);
    } catch {
      return markup;
    }
  }

  if (!value || !Array.isArray(value.inline_keyboard)) return markup;

  return {
    ...value,
    inline_keyboard: value.inline_keyboard.map((row) =>
      row.map((button) => {
        const { icon_custom_emoji_id, ...rest } = button;
        return rest;
      })
    )
  };
}

function stripTextEmojis(payload) {
  const result = { ...payload };

  if (typeof result.text === 'string') {
    result.text = result.text.replace(
      /<tg-emoji\b[^>]*>([\s\S]*?)<\/tg-emoji>/gi,
      '$1'
    );
  }

  if (Array.isArray(result.entities)) {
    result.entities = result.entities.filter(
      (entity) => entity.type !== 'custom_emoji'
    );
  }

  return result;
}

bot.api.config.use(async (prev, method, payload) => {
  // Ловим битые клавиатуры ДО запроса к Telegram: именно из-за них кнопки
  // перестают реагировать (callback_data подменяется текстом кнопки).
  if (payload && payload.reply_markup) {
    assertKeyboard(payload.reply_markup, `${method} reply_markup`);
  }

  let response = await prev(method, payload);

  if (response.ok || !RETRY_METHODS.has(method)) return response;
  if (!RETRYABLE_ERROR.test(response.description || '')) return response;

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

    response = await prev(method, next);

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

  const url = `https://api.telegram.org/file/bot${BOT_TOKEN}/${encodeURI(file.file_path)}`;
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
