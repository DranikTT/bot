'use strict';

/**
 * Защита от «мёртвых» inline-кнопок.
 *
 * Главная ловушка grammY: у `InlineKeyboard#text()` вторым аргументом идёт
 * callback_data, а объект в первом аргументе описывает только отображение
 * (text, icon_custom_emoji_id, style). Если положить `callback_data` внутрь
 * объекта, grammY молча перезапишет его текстом кнопки:
 *
 *   new InlineKeyboard().text({ text: 'Настройки', callback_data: 'menu_settings' })
 *   // → { text: 'Настройки', callback_data: 'Настройки' }
 *
 * Бот получает вместо кода строку «Настройки», ни один обработчик не
 * совпадает, и кнопка выглядит нерабочей. Поэтому все кнопки собираются
 * через callbackButton(), а findKeyboardProblems() страхует остальное.
 */

const MAX_CALLBACK_DATA_BYTES = 64;

const VALID_CALLBACK_DATA = /^[A-Za-z0-9_:.-]{1,64}$/;

const BUTTON_ACTION_FIELDS = [
  'callback_data',
  'url',
  'web_app',
  'login_url',
  'switch_inline_query',
  'switch_inline_query_current_chat',
  'switch_inline_query_chosen_chat',
  'copy_text',
  'pay',
  'game_short_name',
  'callback_game'
];

function callbackButton(text, callbackData, options = {}) {
  const data = String(callbackData ?? '');

  if (!data) {
    throw new TypeError(`Пустой callback_data у кнопки «${text}»`);
  }

  if (Buffer.byteLength(data, 'utf8') > MAX_CALLBACK_DATA_BYTES) {
    throw new RangeError(`callback_data кнопки «${text}» длиннее 64 байт`);
  }

  return { ...options, text: String(text), callback_data: data };
}

function getKeyboardRows(markup) {
  if (!markup) return null;

  let value = markup;

  if (typeof value === 'string') {
    try {
      value = JSON.parse(value);
    } catch {
      return null;
    }
  }

  if (!value || typeof value !== 'object' || !Array.isArray(value.inline_keyboard)) {
    return null;
  }

  return value.inline_keyboard;
}

function findKeyboardProblems(markup) {
  const rows = getKeyboardRows(markup);
  if (!rows) return [];

  const problems = [];

  rows.forEach((row, rowIndex) => {
    (Array.isArray(row) ? row : []).forEach((button, buttonIndex) => {
      if (!button || typeof button !== 'object') return;

      const label = `[${rowIndex}:${buttonIndex}] «${button.text}»`;
      const hasAction = BUTTON_ACTION_FIELDS.some(
        (field) => button[field] !== undefined && button[field] !== null
      );

      if (!hasAction) {
        problems.push(`${label}: нет callback_data/url — нажатие ничего не сделает`);
        return;
      }

      if (button.callback_data === undefined) return;

      const data = String(button.callback_data);

      if (!VALID_CALLBACK_DATA.test(data)) {
        problems.push(
          `${label}: callback_data «${data}» содержит недопустимые символы (похоже, туда попал текст кнопки)`
        );
      }

      if (data === String(button.text)) {
        problems.push(
          `${label}: callback_data совпадает с текстом кнопки — обработчик не найдётся`
        );
      }
    });
  });

  return problems;
}

function assertKeyboard(markup, context = 'keyboard') {
  const problems = findKeyboardProblems(markup);

  if (problems.length) {
    console.error(`[${context}] Некорректная клавиатура:\n - ${problems.join('\n - ')}`);
  }

  return problems;
}

module.exports = {
  callbackButton,
  findKeyboardProblems,
  assertKeyboard,
  getKeyboardRows
};
