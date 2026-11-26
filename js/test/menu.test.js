#!/usr/bin/env node
/**
 * Проверка меню и клавиатур без сети: эмулируем Telegram-объекты.
 *
 * Запуск: node test/menu.test.js
 */
'use strict';

const assert = require('assert');
const path = require('path');

process.env.BOT_TOKEN = process.env.BOT_TOKEN || '1:test';
process.env.ADMIN_IDS = '42';
process.env.DATABASE_PATH = path.join(__dirname, 'test_bot.sqlite');

const keys = require('../menuKeys');

let passed = 0;
let failed = 0;

// Асинхронные тесты запускаются строго по очереди и только в конце:
// они меняют общую заглушку базы, параллельный запуск перетирал бы её.
const asyncQueue = [];

function testAsync(name, fn) {
  asyncQueue.push(async () => {
    try {
      await fn();
      passed++;
      console.log(`  ✔ ${name}`);
    } catch (error) {
      failed++;
      console.log(`  ✘ ${name}`);
      console.log(`      ${error.message}`);
    }
  });
}

function test(name, fn) {
  const record = (error) => {
    if (error) {
      failed++;
      console.log(`  ✘ ${name}`);
      console.log(`      ${error.message}`);
      return;
    }

    passed++;
    console.log(`  ✔ ${name}`);
  };

  try {
    const result = fn();

    if (result && typeof result.then === 'function') {
      record(new Error('async-тест объявлен через test() — используйте testAsync()'));
      return;
    }

    record(null);
  } catch (error) {
    record(error);
  }
}

console.log('\nКлавиатуры');

function flat(keyboard) {
  return keyboard.inline_keyboard.flat();
}

test('callback_data не подменяется текстом (главный баг grammy)', () => {
  const keyboard = keys.getMainMenuKeyboard(1);
  for (const button of flat(keyboard)) {
    assert.ok(button.callback_data, `у кнопки «${button.text}» нет callback_data`);
    assert.notStrictEqual(
      button.callback_data,
      button.text,
      `callback_data совпал с текстом у «${button.text}» — grammy съел данные`
    );
  }
});

test('главное меню — исходный дизайн', () => {
  const keyboard = keys.getMainMenuKeyboard(1);
  const rows = keyboard.inline_keyboard.map((row) => row.map((b) => b.text));

  assert.deepStrictEqual(rows[0], ['Настройки', 'Подключить бота']);
  assert.deepStrictEqual(rows[1], ['Пригласи друга']);
  assert.deepStrictEqual(rows[2], ['Подписка']);
  assert.strictEqual(rows.length, 3, 'админу не должно быть лишних рядов');
});

test('админ видит «Админ-панель»', () => {
  const keyboard = keys.getMainMenuKeyboard(42); // 42 в ADMIN_IDS
  const texts = flat(keyboard).map((b) => b.text);

  assert.ok(texts.includes('Админ-панель'), 'нет кнопки админки');
  assert.strictEqual(keyboard.inline_keyboard.length, 4);
});

test('обычный пользователь админку не видит', () => {
  const keyboard = keys.getMainMenuKeyboard(999);
  const texts = flat(keyboard).map((b) => b.text);

  assert.ok(!texts.includes('Админ-панель'));
});

test('все кнопки несут кастомные эмодзи', () => {
  const keyboards = [
    keys.getMainMenuKeyboard(42),
    keys.getSettingsKeyboard(1),
    keys.getConnectKeyboard(),
    keys.getSubKeyboard(),
    keys.getPremiumKeyboard(),
    keys.getRefKeyboard(),
    keys.getAdminKeyboard()
  ];

  for (const keyboard of keyboards) {
    for (const button of flat(keyboard)) {
      assert.ok(
        /^\d+$/.test(String(button.icon_custom_emoji_id)),
        `кнопка «${button.text}» без кастомного эмодзи`
      );
    }
  }
});

test('кнопки имеют стили primary/success/danger', () => {
  const styles = new Set(
    flat(keys.getMainMenuKeyboard(42)).map((button) => button.style)
  );

  assert.ok(styles.has('primary'));
  assert.ok(styles.has('danger'), 'админка должна быть danger');

  const subStyles = new Set(
    flat(keys.getSubKeyboard()).map((button) => button.style)
  );
  assert.ok(subStyles.has('success'), 'премиум должен быть success');
});

test('callback_data умещается в лимит Telegram (64 байта)', () => {
  for (const button of flat(keys.getAdminKeyboard())) {
    assert.ok(
      Buffer.byteLength(button.callback_data, 'utf8') <= 64,
      `слишком длинный callback_data: ${button.callback_data}`
    );
  }
});

test('нет кнопки «Создать бота»', () => {
  const texts = flat(keys.getMainMenuKeyboard(42)).map((b) => b.text);
  assert.ok(!texts.some((text) => /создать бот/i.test(text)));
});

console.log('\nЭмодзи');

test('ID эмодзи — настоящие 16+ цифр', () => {
  for (const [name, id] of Object.entries(keys.EMOJI)) {
    assert.ok(/^\d{15,20}$/.test(id), `${name}: ${id}`);
  }
});

test('все ID уникальны', () => {
  const ids = Object.values(keys.EMOJI);
  assert.strictEqual(new Set(ids).size, ids.length, 'есть дубли ID');
});

console.log('\nАдрес Bot API');

test('apiRoot не содержит /bot (grammy добавит его сам)', () => {
  // Если передать apiRoot с /bot, URL станет /bot/bot<token>/getMe,
  // и Telegram ответит 404 Not Found — бот не стартует.
  const config = require('../config');
  assert.ok(config.API_BASE, 'API_BASE не экспортируется');
  assert.ok(
    !/\/bot$/.test(config.API_BASE),
    `API_BASE заканчивается на /bot — будет двойной /bot/bot: ${config.API_BASE}`
  );
});

test('адрес Bot API по умолчанию — официальный', () => {
  const config = require('../config');
  assert.ok(
    config.API_BASE === 'https://api.telegram.org' ||
      config.API_BASE.startsWith('http'),
    config.API_BASE
  );
});

test('все загрузки файлов идут через URL, а не в обход', () => {
  const fs = require('fs');
  const path = require('path');

  for (const file of ['bot.js', 'bussines.js']) {
    const source = fs.readFileSync(path.join(__dirname, '..', file), 'utf8');
    assert.ok(
      !source.includes('https://api.telegram.org/file/bot'),
      `${file} жёстко ходит в api.telegram.org — сломается на локальном Bot API`
    );
  }
});

console.log('\nМеню');

// Заглушка БД: меню не должно требовать реального SQLite для текстов
const dbPath = require.resolve('../database');
const realDb = require.cache[dbPath];

// menus подтягивает database — подменяем на лёгкую заглушку
function fakeUser(overrides = {}) {
  return {
    user_id: 7,
    first_name: 'Вася',
    username: 'vasya',
    subscription_until: Math.floor(Date.now() / 1000) + 10 * 86400,
    is_premium: 0,
    is_banned: 0,
    referrals_count: 3,
    notify_deleted: 1,
    notify_edited: 1,
    ...overrides
  };
}

// Меню проверяем на текстах, подменив db через require cache
const dbStub = {
  getUser: () => fakeUser(),
  getBusinessConnectionByOwner: () => ({ is_enabled: 1, connection_id: 'c1' }),
  getStats: () => ({
    totalUsers: 10,
    activeSubs: 4,
    premiumUsers: 2,
    bannedUsers: 1,
    activeConnections: 3
  }),
  hasPremium: () => true
};

require.cache[dbPath] = {
  id: dbPath,
  filename: dbPath,
  loaded: true,
  exports: dbStub
};

const menus = require('../menus');

test('главное меню: приветствие и кастомные эмодзи', () => {
  const menu = menus.getMainMenu({ id: 7, first_name: 'Вася', username: 'vasya' });

  assert.ok(menu.text.includes('Привет'), menu.text);
  assert.ok(menu.text.includes('<tg-emoji'), 'нет кастомных эмодзи');
  assert.ok(menu.text.includes('</tg-emoji>'));
  assert.strictEqual(menu.parse_mode, 'HTML');
});

test('главное меню: баннер mainmenu.jpg', () => {
  const menu = menus.getMainMenu({ id: 7, first_name: 'Вася' });
  assert.ok(menu.banner, 'нет баннера');
  assert.ok(menu.banner.endsWith('mainmenu.jpg'), menu.banner);
  assert.strictEqual(menu.bannerKey, 'main');
});

test('настройки: статусы ВКЛ/ВЫКЛ', () => {
  const menu = menus.getSettingsMenu(7);

  assert.ok(menu.text.includes('Настройки'));
  assert.ok(menu.text.includes('Включены'), menu.text);
  const toggles = menu.reply_markup.inline_keyboard[0].map((b) => b.text);
  assert.ok(toggles[0].includes('ВКЛ'), toggles[0]);
});

test('подписка: баннер subscription.jpg и статус', () => {
  const menu = menus.getSubMenu(7);

  assert.ok(menu.banner.endsWith('subscription.jpg'), menu.banner);
  assert.ok(menu.text.includes('активна до'), menu.text);
  assert.ok(menu.text.includes('10 дн.'), menu.text);
});

test('бан статуса подписки', () => {
  dbStub.getUser = () => fakeUser({ is_banned: 1 });
  const menu = menus.getSubMenu(7);
  assert.ok(menu.text.includes('заблокирована'), menu.text);
  dbStub.getUser = () => fakeUser();
});

test('премиум: статус и годовой тариф', () => {
  const menu = menus.getPremiumMenu(7);

  assert.ok(menu.text.includes('Активен'), menu.text);
  const buttons = menu.reply_markup.inline_keyboard.flat();
  assert.ok(buttons.some((b) => b.callback_data === 'buy_sub_premium'));
});

test('рефералка: ссылка с id и счётчиком', () => {
  const menu = menus.getRefMenu(7);

  assert.ok(menu.text.includes('start=ref_7'), menu.text);
  assert.ok(menu.text.includes('Приглашено'), menu.text);
});

test('подключение: инструкция с юзернеймом', () => {
  const menu = menus.getConnectMenu();

  assert.ok(menu.text.includes('Автоматизация'), menu.text);
  assert.ok(menu.text.includes('<code>@'), 'нет @username бота');
  assert.ok(menu.text.includes('<tg-emoji'), 'нет эмодзи');
});

test('админка: баннер adminmenu.jpg и статистика', () => {
  const menu = menus.getAdminMenu();

  assert.ok(menu.banner.endsWith('adminmenu.jpg'), menu.banner);
  assert.ok(menu.text.includes('Всего пользователей'), menu.text);
  assert.ok(menu.text.includes('<b>10</b>'), 'нет цифры пользователей');
});

// --- замена меню: главная защита от дублей -------------------------------

function fakeApi({ business = null, deleteFails = null } = {}) {
  const deleted = [];
  const sent = [];

  // Заглушку базы ставим в момент запуска теста, а не при его объявлении:
  // async-тесты выполняются позже, и «последний объявленный» перетёр бы
  // настройку всех остальных.
  const activate = () => {
    dbStub.getBusinessConnectionByOwner = () => business;
  };

  const api = {
    deleteMessage: async (payload) => {
      // как настоящий Telegram: в Business-чате без business_connection_id
      // сообщение не удаляется
      const active = business && Number(business.is_enabled) === 1;

      if (active && !payload.business_connection_id) {
        const error = new Error("Bad Request: message can't be deleted");
        error.description = "Bad Request: message can't be deleted";
        throw error;
      }

      if (deleteFails) throw deleteFails;

      deleted.push({
        chatId: payload.chat_id,
        messageId: payload.message_id,
        conn: payload.business_connection_id
      });
      return true;
    },
    sendMessage: async (chatId, text, options = {}) => {
      sent.push({ chatId, text, conn: options.business_connection_id });
      return { message_id: 500, chat: { id: chatId }, text };
    },
    sendPhoto: async () => {
      throw new Error('баннер в тесте не отправляется');
    }
  };

  return { api, deleted, sent, activate };
}

const MENU = { text: 'Тестовое меню', banner: null, reply_markup: { inline_keyboard: [] } };

testAsync('Business: удаление идёт с business_connection_id (нет дублей)', async () => {
  const { api, deleted, sent, activate } = fakeApi({
    business: { user_id: 900, connection_id: 'conn-abc', is_enabled: 1 }
  });
  activate();

  await menus.replaceMenu({ api }, 900, 11, MENU, { business_connection_id: 'conn-abc' });

  assert.strictEqual(deleted.length, 1, 'старое меню не удалено — будут дубли');
  assert.strictEqual(deleted[0].messageId, 11);
  assert.strictEqual(
    deleted[0].conn,
    'conn-abc',
    'deleteMessage ушёл без business_connection_id — в Business-чате не сработает'
  );
  assert.strictEqual(sent.length, 1, 'новое меню не отправлено');
});

testAsync('Business-чат клиента: connection_id берётся из обновления, не из базы', async () => {
  // база знает только владельца 900; чат клиента 555 — другое лицо
  const { api, deleted, sent, activate } = fakeApi({
    business: { user_id: 900, connection_id: 'conn-abc', is_enabled: 1 }
  });
  activate();

  await menus.replaceMenu({ api }, 555, 21, MENU, { business_connection_id: 'conn-abc' });

  assert.strictEqual(deleted.length, 1, 'старое меню в чате клиента не удалено');
  assert.strictEqual(
    deleted[0].conn,
    'conn-abc',
    'удаление в чате клиента ушло без business_connection_id — будут дубли'
  );
  assert.strictEqual(sent[0].conn, 'conn-abc', 'новое меню в чате клиента без connection_id');
});

testAsync('вне Business: обычное удаление без connection_id', async () => {
  const { api, deleted, sent, activate } = fakeApi({ business: null });
  activate();

  await menus.replaceMenu({ api }, 901, 12, MENU);

  assert.strictEqual(deleted.length, 1);
  assert.strictEqual(deleted[0].messageId, 12);
  assert.strictEqual(deleted[0].conn, undefined, 'лишний business_connection_id');
  assert.strictEqual(sent.length, 1);
});

testAsync('Business отключён: шлём и удаляем как обычный чат', async () => {
  const { api, deleted, sent, activate } = fakeApi({
    business: { user_id: 903, connection_id: 'conn-off', is_enabled: 0 }
  });
  activate();

  await menus.replaceMenu({ api }, 903, 14, MENU);

  assert.strictEqual(deleted.length, 1);
  assert.strictEqual(deleted[0].conn, undefined);
  assert.strictEqual(sent[0].conn, undefined);
});

testAsync('пропавшее сообщение: ошибка не мешает отправке нового', async () => {
  const err = new Error('Bad Request: message to delete not found');
  err.description = 'Bad Request: message to delete not found';

  const { api, sent, activate } = fakeApi({ business: null, deleteFails: err });
  activate();

  await menus.replaceMenu({ api }, 902, 13, MENU);

  assert.strictEqual(sent.length, 1, 'новое меню не отправлено после «not found»');
});

testAsync('меню не дублируется: 5 переходов → 5 удалений и 5 отправок', async () => {
  const { api, deleted, sent, activate } = fakeApi({
    business: { user_id: 904, connection_id: 'conn-x', is_enabled: 1 }
  });
  activate();

  let currentId = 100;

  for (let step = 0; step < 5; step++) {
    const msg = await menus.replaceMenu({ api }, 904, currentId, MENU, {
      business_connection_id: 'conn-x'
    });

    currentId = msg.message_id;
  }

  assert.strictEqual(deleted.length, 5, `удалено ${deleted.length}, ожидалось 5`);
  assert.strictEqual(sent.length, 5, `отправлено ${sent.length}, ожидалось 5`);
  assert.ok(
    deleted.every((call) => call.conn === 'conn-x'),
    'хотя бы одно удаление без business_connection_id'
  );
});

test('архив: типы сообщений с переключателями', () => {
  const menu = menus.getArchiveTypesMenu('555', 'Собеседник', new Set(['text', 'photo']));
  const buttons = menu.reply_markup.inline_keyboard.flat();

  const textButton = buttons.find((b) => b.callback_data === 'arch_toggle_text');
  assert.ok(textButton, 'нет кнопки text');
  assert.ok(textButton.text.includes('✅'), 'выбранный тип без галочки');

  const videoButton = buttons.find((b) => b.callback_data === 'arch_toggle_video');
  assert.ok(videoButton.text.includes('❌'), 'невыбранный тип с галочкой');
});

test('HTML в меню экранируется', () => {
  dbStub.getUser = () => fakeUser({ first_name: '<script>alert(1)</script>' });
  const menu = menus.getMainMenu({ id: 7, first_name: '<script>alert(1)</script>' });

  assert.ok(!menu.text.includes('<script>'), 'XSS не экранирован');
  assert.ok(menu.text.includes('&lt;script&gt;'), menu.text);
  dbStub.getUser = () => fakeUser();
});

test('формат даты подписки', () => {
  const formatted = menus.formatDateTime(1700000000);
  assert.ok(/^\d{2}\.\d{2}\.\d{4} \d{2}:\d{2}$/.test(formatted), formatted);
});

if (realDb) require.cache[dbPath] = realDb;

(async () => {
  for (const run of asyncQueue) await run();

  console.log(`\nИтого: ${passed} прошло, ${failed} упало\n`);
  process.exit(failed === 0 ? 0 : 1);
})();
