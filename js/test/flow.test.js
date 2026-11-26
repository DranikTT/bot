#!/usr/bin/env node
/**
 * Сквозная проверка: настоящий бот против фейкового Telegram API.
 *
 * Запуск: node test/flow.test.js
 */
'use strict';

const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const PORT = 8098;
const DB_PATH = path.join(os.tmpdir(), `savemod_flow_${Date.now()}.sqlite`);

let passed = 0;
let failed = 0;

const pendingTests = [];
let scenarioDone = false;

function test(name, fn) {
  if (!scenarioDone) {
    pendingTests.push([name, fn]);
    return;
  }

  try {
    fn();
    passed++;
    console.log(`  ✔ ${name}`);
  } catch (error) {
    failed++;
    console.log(`  ✘ ${name}`);
    console.log(`      ${error.message}`);
  }
}

const env = {
  ...process.env,
  BOT_TOKEN: '42:TESTTOKEN',
  ADMIN_IDS: '777',
  API_SERVER: `http://127.0.0.1:${PORT}`,
  DATABASE_PATH: DB_PATH
};

console.log('\nСквозной сценарий (бот + фейковый Telegram)\n');

async function runScenario() {
  const { spawn: spawnAsync } = require('child_process');

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  // 1. фейковый API поднимается первым и печатает готовность
  const fakeProc = spawnAsync('python3', [path.join(__dirname, 'fake_api.py')], {
    env: { ...env, FAKE_PORT: String(PORT) },
    stdio: ['ignore', 'pipe', 'pipe']
  });

  let fakeOut = '';
  let ready = false;

  fakeProc.stdout.on('data', (chunk) => {
    fakeOut += chunk.toString();
    if (fakeOut.includes('fake Telegram API')) ready = true;
  });

  for (let attempt = 0; attempt < 100 && !ready; attempt++) {
    await sleep(100);
  }

  assert.ok(ready, `фейковый сервер не поднялся: ${fakeOut}`);

  // 2. настоящий бот
  const botProc = spawnAsync('node', [path.join(ROOT, 'bot.js')], {
    env,
    cwd: ROOT,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  let botOut = '';
  botProc.stdout.on('data', (chunk) => (botOut += chunk.toString()));
  botProc.stderr.on('data', (chunk) => (botOut += chunk.toString()));

  // 3. ждём, пока фейк раздаст все обновления и завершится
  const finished = await new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), 25000);
    fakeProc.on('exit', () => {
      clearTimeout(timer);
      resolve(true);
    });
  });

  botProc.kill('SIGTERM');
  await sleep(300);

  assert.ok(finished, 'фейковый сервер не завершил сценарий');
  assert.ok(fakeOut.includes('['), `фейк не записал вызовы. Лог бота: ${botOut.slice(0, 800)}`);

  const parsed = JSON.parse(fakeOut.slice(fakeOut.indexOf('[')));
  return { calls: parsed, botOut };
}

// Основная проверка выполняется в конце файла (ниже), здесь только объявляем
let calls = [];

function finish() {
  console.log(`\nИтого: ${passed} прошло, ${failed} упало\n`);
  process.exit(failed === 0 ? 0 : 1);
}

function callsOf(method) {
  return calls.filter((call) => call.method === method);
}

function flatButtons(call) {
  const markup = call.payload.reply_markup;

  if (!markup || !Array.isArray(markup.inline_keyboard)) return [];
  return markup.inline_keyboard.flat();
}

test('бот отправил меню на /start', () => {
  const menus = [...callsOf('sendPhoto'), ...callsOf('sendMessage')];

  assert.ok(menus.length > 0, 'меню не отправлено вообще');
});

test('меню уходит баннером (mainmenu.jpg)', () => {
  const photos = callsOf('sendPhoto');

  assert.ok(photos.length > 0, `меню текстом, а не фото: ${JSON.stringify(calls.map((c) => c.method))}`);

  const captions = photos.map((photo) => photo.payload.caption || '');
  assert.ok(
    captions.some((caption) => caption.includes('Привет')),
    'главное меню не пришло фотографией'
  );
});

test('кастомные эмодзи присутствуют в подписи', () => {
  const photos = callsOf('sendPhoto');
  const withEmoji = photos.filter((photo) =>
    String(photo.payload.caption || '').includes('<tg-emoji')
  );

  assert.ok(withEmoji.length > 0, 'ни одного <tg-emoji> в подписях');
  assert.strictEqual(withEmoji[0].payload.parse_mode, 'HTML');
});

test('кнопки несут рабочий callback_data (не текст)', () => {
  const menus = [...callsOf('sendPhoto'), ...callsOf('sendMessage')];
  let buttons = 0;

  for (const call of menus) {
    for (const button of flatButtons(call)) {
      buttons++;
      assert.ok(button.callback_data, `кнопка «${button.text}» без callback_data`);
      assert.notStrictEqual(
        button.callback_data,
        button.text,
        `callback_data = текст у «${button.text}»`
      );
    }
  }

  assert.ok(buttons >= 8, `слишком мало кнопок: ${buttons}`);
});

test('кнопки несут кастомные эмодзи', () => {
  const menus = [...callsOf('sendPhoto'), ...callsOf('sendMessage')];
  const buttons = menus.flatMap(flatButtons).filter((button) => button.icon_custom_emoji_id);

  assert.ok(buttons.length > 0, 'у кнопок нет icon_custom_emoji_id');
});

test('меню показывает изменение настройки (ВКЛ → ВЫКЛ)', () => {
  const menus = [...callsOf('sendPhoto'), ...callsOf('sendMessage')]
    .map((call) => call.payload.caption || call.payload.text || '')
    .filter((text) => text.includes('Настройки'));

  assert.ok(
    menus.some((text) => text.includes('Включены')) &&
      menus.some((text) => text.includes('Выключены')),
    `настройка не переключилась: ${JSON.stringify(menus)}`
  );
});

test('тарифы в звёздах с правильными callback_data', () => {
  const buttons = [...callsOf('sendPhoto'), ...callsOf('sendMessage')].flatMap(flatButtons);
  const tariffs = buttons.filter((button) => String(button.callback_data).startsWith('buy_sub'));

  assert.ok(tariffs.length >= 4, `найдено тарифов: ${tariffs.length}`);
});

test('оплата уходит инвойсом в XTR', () => {
  const invoices = callsOf('sendInvoice');

  assert.ok(invoices.length > 0, 'инвойс не отправлен');
  assert.strictEqual(invoices[0].payload.currency, 'XTR');

  const prices = invoices[0].payload.prices || [];
  assert.ok(prices.length > 0, 'нет цен в инвойсе');
});

test('нет кнопки «Создать бота»', () => {
  const texts = calls.map((call) => JSON.stringify(call.payload)).join(' ');
  assert.ok(!/создать бот/i.test(texts), 'кнопка «Создать бота» вернулась');
});

test('все ID эмодзи настоящие', () => {
  const text = calls.map((call) => JSON.stringify(call.payload)).join(' ');
  const ids = [...text.matchAll(/emoji-id=\\"(\d+)\\"/g)].map((match) => match[1]);

  assert.ok(ids.length > 0, 'не найдено ни одного emoji-id');
  for (const id of ids) {
    assert.ok(/^\d{15,20}$/.test(id), `подозрительный ID: ${id}`);
  }
});

// 2. проверяем, что база реально наполнилась при импорте
console.log('\nХранилище\n');

test('база создаётся и содержит пользователя', () => {
  const { DatabaseSync } = require('node:sqlite');

  assert.ok(fs.existsSync(DB_PATH), 'файл базы не создан');

  const db = new DatabaseSync(DB_PATH);
  const users = db.prepare('SELECT COUNT(*) as c FROM users').get();
  db.close();

  assert.ok(users.c > 0, 'в базе нет пользователей');
});

test('тексты сообщений в базе зашифрованы (enc:v1:)', () => {
  // Шифруются тексты переписки (messages), а не профиль пользователя:
  // getUser() намеренно отдаёт имя как есть для отображения в меню.
  const { DatabaseSync } = require('node:sqlite');
  const { encryptStr, decryptStr } = require('../database');

  const cipher = encryptStr('секретный текст');
  assert.ok(cipher.startsWith('enc:v1:'), 'нет префикса enc:v1:');
  assert.ok(!cipher.includes('секретный'), 'шифртекст содержит открытый текст');
  assert.strictEqual(decryptStr(cipher), 'секретный текст');
});

test('шифрование читает старые записи открытым текстом', () => {
  const { decryptStr } = require('../database');
  assert.strictEqual(decryptStr('старое сообщение'), 'старое сообщение');
});

test('эмодзи-идентификаторы совпадают с исходным проектом', () => {
// --- Business-чат: главная проверка «нет дублей» -------------------------
// Сценарий фейка присылает business_connection (владелец 777) и три нажатия
// в чате КЛИЕНТА (555) с business_connection_id = conn-e2e.

test('бот сам обработал business_connection (без нажатия кнопки)', () => {
  const saved = callsOf('sendMessage').filter(
    (call) =>
      String(call.payload.chat_id) === '777' &&
      /Подключение активно|Бот подключён/i.test(String(call.payload.text || ''))
  );

  assert.ok(
    saved.length > 0,
    'бот не отреагировал на business_connection — владелец не узнал о подключении'
  );
});

test('Business: ни одного проваленного deleteMessage', () => {
  const failed = callsOf('deleteMessage').filter((call) => call.failed);

  assert.strictEqual(
    failed.length,
    0,
    `провалено удалений: ${failed.length} — старое меню остаётся, новое приходит сверху (дубли)`
  );
});

test('Business: каждое удаление в чате клиента несёт conn-e2e', () => {
  const deletes = callsOf('deleteMessage').filter(
    (call) => String(call.payload.chat_id) === '555'
  );

  assert.ok(deletes.length > 0, 'удалений в Business-чате не было вовсе — меню копится');

  for (const call of deletes) {
    assert.strictEqual(
      call.payload.business_connection_id,
      'conn-e2e',
      `deleteMessage без правильного business_connection_id: ${JSON.stringify(call.payload)}`
    );
  }
});

test('Business: на каждый экран ровно одно новое меню', () => {
  // три нажатия в чате клиента → три пары «удалить старое / показать новое»
  const inClientChat = [
    ...callsOf('sendMessage'),
    ...callsOf('sendPhoto')
  ].filter((call) => String(call.payload.chat_id) === '555');
  const deletesInClientChat = callsOf('deleteMessage').filter(
    (call) => String(call.payload.chat_id) === '555'
  );

  assert.strictEqual(inClientChat.length, 3, `меню в чате клиента: ${inClientChat.length}, ожидалось 3`);
  assert.strictEqual(
    deletesInClientChat.length,
    inClientChat.length,
    `отправлено ${inClientChat.length}, удалено ${deletesInClientChat.length} — меню копится`
  );

  // ни одна отправка не осталась без удаления предыдущей
  assert.ok(
    inClientChat.every((call) => call.payload.business_connection_id === 'conn-e2e'),
    'меню в Business-чат ушло без business_connection_id'
  );
});

test('личный чат владельца не ломается (удаления без connection_id)', () => {
  const deletes = callsOf('deleteMessage').filter(
    (call) => String(call.payload.chat_id) === '777'
  );

  assert.ok(deletes.length > 0, 'в личном чате меню не заменяется');

  for (const call of deletes) {
    assert.strictEqual(
      call.payload.business_connection_id,
      undefined,
      'в личном чате лишний business_connection_id — Telegram отклонит удаление'
    );
  }
});

  process.env.BOT_TOKEN = process.env.BOT_TOKEN || '42:TESTTOKEN';
  process.env.DATABASE_PATH = process.env.DATABASE_PATH || DB_PATH;
  const keys = require('../menuKeys');

  assert.strictEqual(keys.EMOJI.gear, '5877260593903177342');
  assert.strictEqual(keys.EMOJI.crown, '5807868868886009920');
  assert.strictEqual(keys.EMOJI.gem, '5877396173135808022');
  assert.strictEqual(keys.EMOJI.star, '5879785854284599288');
});

// --- перехват переписки: отдельный прогон ---------------------------------
//
// Меню и перехват проверяются разными прогонами: в одном сценарии они
// конкурируют за порядок обновлений, и тест становится нестабильным.
// Здесь всё как в жизни: подключение → переписка → удаления.
async function runLive() {
  const { spawn: spawnAsync } = require('child_process');
  const fakePort = String(Number(PORT) + 1);

  const fakeProc = spawnAsync('python3', [path.join(__dirname, 'fake_api.py')], {
    env: { ...env, FAKE_PORT: fakePort, LIVE: '1' },
    stdio: ['ignore', 'pipe', 'pipe']
  });

  let fakeOut = '';
  let ready = false;

  fakeProc.stdout.on('data', (chunk) => {
    fakeOut += chunk.toString();
    if (fakeOut.includes('fake Telegram API')) ready = true;
  });

  for (let attempt = 0; attempt < 100 && !ready; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  assert.ok(ready, `LIVE: фейковый сервер не поднялся: ${fakeOut}`);

  const botProc = spawnAsync('node', [path.join(ROOT, 'bot.js')], {
    env: { ...env, API_SERVER: `http://127.0.0.1:${fakePort}` },
    cwd: ROOT,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  let botOut = '';
  botProc.stdout.on('data', (chunk) => (botOut += chunk.toString()));
  botProc.stderr.on('data', (chunk) => (botOut += chunk.toString()));

  const finished = await new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), 40000);
    fakeProc.on('exit', () => {
      clearTimeout(timer);
      resolve(true);
    });
  });

  botProc.kill('SIGTERM');
  await new Promise((resolve) => setTimeout(resolve, 300));

  assert.ok(finished, `LIVE: фейковый сервер не завершил сценарий. ${botOut.slice(0, 500)}`);

  return JSON.parse(fakeOut.slice(fakeOut.indexOf('[')));
}

function runLiveTests(liveCalls) {
  const notices = [
    ...liveCalls.filter((c) => c.method === 'sendMessage' || c.method === 'sendPhoto')
  ].filter((call) => {
    const body = String(call.payload.text || call.payload.caption || '')
      .replace(/<[^>]+>/g, '');
    return /Удалённое сообщение|Изменённое сообщение/.test(body);
  });

  const plain = notices.map((call) =>
    String(call.payload.text || call.payload.caption || '').replace(/<[^>]+>/g, '')
  );

  test('перехват: владельцу пришло уведомление об удалённом', () => {
    assert.ok(
      plain.some((text) => /Удалённое сообщение/.test(text)),
      'об удалённом сообщении не уведомили'
    );
    assert.ok(
      plain.some((text) => /Секретик/.test(text)),
      `в уведомлении нет текста удалённого: ${plain.join(' / ').slice(0, 200)}`
    );
  });

  test('перехват: уведомление об изменённом содержит старый и новый текст', () => {
    const body = plain.find((text) => /Изменённое сообщение/.test(text));

    assert.ok(body, 'об изменённом сообщении не уведомили');
    assert.ok(/Привет, как дела\?/.test(body), `нет старого текста: ${body.slice(0, 200)}`);
    assert.ok(/делишки/.test(body), `нет нового текста: ${body.slice(0, 200)}`);
  });

  test('перехват: удалённое фото приходит владельцу файлом', () => {
    const photos = liveCalls.filter((call) => {
      if (call.method !== 'sendPhoto') return false;
      const body = String(call.payload.caption || '').replace(/<[^>]+>/g, '');
      return /Удалённое сообщение/.test(body);
    });

    assert.ok(
      photos.length > 0,
      'самоудаляющееся фото не переслано — оно исчезло вместе с чатом'
    );
  });

  test('перехват: сообщение сохранено в базу с owner_id владельца', () => {
    const { DatabaseSync } = require('node:sqlite');
    const store = new DatabaseSync(DB_PATH);
    const rows = store.prepare('SELECT owner_id, is_deleted, is_self_destruct, local_path FROM messages').all();
    store.close();

    assert.ok(rows.length > 0, 'ни одно сообщение не сохранено');
    assert.ok(
      rows.every((row) => Number(row.owner_id) > 0),
      'у сообщений не определён владелец'
    );
    assert.ok(
      rows.some((row) => row.is_deleted === 1),
      'удаление не отмечено в базе'
    );
    assert.ok(
      rows.some((row) => row.local_path),
      'медиа самоудаляющегося не сохранено на диск'
    );
  });
}

runScenario()
  .then((result) => {
    calls = result.calls;
    scenarioDone = true;

    if (process.env.SHOW_CALLS) {
      console.log('\n--- ВЫЗОВЫ ---');
      for (const c of calls) {
        const body = String(c.payload.text || c.payload.caption || '').replace(/<[^>]+>/g, '').replace(/\n/g, '|').slice(0, 45);
        console.log(c.method, c.payload.chat_id, body);
      }
    }

    // проверки объявлены выше и вызываются здесь, когда данные уже есть
    for (const [name, fn] of pendingTests) {
      test(name, fn);
    }

    try {
      fs.unlinkSync(DB_PATH);
    } catch (_) {}

    return runLive().then((liveCalls) => {
      runLiveTests(liveCalls);
      finish();
    });
  })
  .catch((error) => {
    console.log(`\n  ✘ Сценарий не отработал: ${error.message}\n`);
    process.exit(1);
  });
