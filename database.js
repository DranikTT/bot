'use strict';

const path = require('path');
const crypto = require('crypto');
const Database = require('better-sqlite3');

const db = new Database(path.join(__dirname, 'bot_database.sqlite'));

db.pragma('journal_mode = WAL');
db.pragma('synchronous = NORMAL');
db.pragma('busy_timeout = 5000');

const SECRET_SEED =
  process.env.DB_ENCRYPTION_KEY ||
  'SaveMod_AES256_Default_Secret_Key_2025_Secure!';

const CIPHER_KEY = crypto.createHash('sha256').update(String(SECRET_SEED)).digest();
const ENC_PREFIX = 'enc:v1:';
const REGISTRATION_BONUS_DAYS = 3;
const REFERRAL_BONUS_DAYS = 7;

function encryptStr(plainText) {
  if (plainText === null || plainText === undefined || plainText === '') return '';

  const str = String(plainText);
  if (str.startsWith(ENC_PREFIX)) return str;

  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv('aes-256-gcm', CIPHER_KEY, iv);
  const encrypted = Buffer.concat([cipher.update(str, 'utf8'), cipher.final()]);

  return `${ENC_PREFIX}${iv.toString('base64')}:${cipher
    .getAuthTag()
    .toString('base64')}:${encrypted.toString('base64')}`;
}

function decryptStr(cipherText) {
  if (cipherText === null || cipherText === undefined || cipherText === '') {
    return '';
  }

  const str = String(cipherText);
  if (!str.startsWith(ENC_PREFIX)) return str;

  try {
    const parts = str.slice(ENC_PREFIX.length).split(':');
    if (parts.length !== 3) return '';

    const decipher = crypto.createDecipheriv(
      'aes-256-gcm',
      CIPHER_KEY,
      Buffer.from(parts[0], 'base64')
    );

    decipher.setAuthTag(Buffer.from(parts[1], 'base64'));

    return Buffer.concat([
      decipher.update(Buffer.from(parts[2], 'base64')),
      decipher.final()
    ]).toString('utf8');
  } catch {
    return '';
  }
}

function decryptMsgRow(row) {
  if (!row) return null;

  return {
    ...row,
    sender_name: decryptStr(row.sender_name),
    sender_username: decryptStr(row.sender_username),
    peer_name: decryptStr(row.peer_name),
    peer_username: decryptStr(row.peer_username),
    text: decryptStr(row.text),
    edit_history: decryptStr(row.edit_history),
    reactions_history: decryptStr(row.reactions_history),
    gift_json: decryptStr(row.gift_json),
    file_name: decryptStr(row.file_name),
    local_path: row.local_path ? decryptStr(row.local_path) : null
  };
}

db.exec(`
  CREATE TABLE IF NOT EXISTS users (
    user_id INTEGER PRIMARY KEY,
    username TEXT DEFAULT '',
    first_name TEXT DEFAULT '',
    subscription_until INTEGER DEFAULT 0,
    is_premium INTEGER DEFAULT 0,
    trial_used INTEGER DEFAULT 0,
    notify_deleted INTEGER DEFAULT 1,
    notify_edited INTEGER DEFAULT 1,
    referred_by INTEGER DEFAULT NULL,
    referrals_count INTEGER DEFAULT 0,
    is_banned INTEGER DEFAULT 0,
    ban_reason TEXT DEFAULT '',
    mtproto_session TEXT DEFAULT '',
    mtproto_phone TEXT DEFAULT '',
    created_at INTEGER DEFAULT 0
  );

  CREATE TABLE IF NOT EXISTS muted_chats (
    owner_id INTEGER,
    chat_id INTEGER,
    PRIMARY KEY (owner_id, chat_id)
  );

  CREATE TABLE IF NOT EXISTS bypass_chats (
    owner_id INTEGER,
    chat_id INTEGER,
    PRIMARY KEY (owner_id, chat_id)
  );

  CREATE TABLE IF NOT EXISTS business_connections (
    connection_id TEXT PRIMARY KEY,
    user_id INTEGER,
    user_chat_id INTEGER,
    can_reply INTEGER DEFAULT 0,
    is_enabled INTEGER DEFAULT 1,
    updated_at INTEGER DEFAULT 0
  );

  CREATE TABLE IF NOT EXISTS messages (
    chat_id INTEGER,
    message_id INTEGER,
    connection_id TEXT DEFAULT '',
    owner_id INTEGER,
    sender_id INTEGER,
    sender_name TEXT DEFAULT '',
    sender_username TEXT DEFAULT '',
    peer_name TEXT DEFAULT '',
    peer_username TEXT DEFAULT '',
    text TEXT DEFAULT '',
    edit_history TEXT DEFAULT '[]',
    reactions_history TEXT DEFAULT '[]',
    gift_json TEXT DEFAULT '',
    media_type TEXT DEFAULT NULL,
    file_id TEXT DEFAULT NULL,
    file_name TEXT DEFAULT NULL,
    local_path TEXT DEFAULT NULL,
    is_self_destruct INTEGER DEFAULT 0,
    is_deleted INTEGER DEFAULT 0,
    date INTEGER DEFAULT 0,
    PRIMARY KEY (chat_id, message_id)
  );
`);

function ensureColumn(tableName, columnName, columnDef) {
  const columns = db.prepare(`PRAGMA table_info(${tableName})`).all();

  if (!columns.some((column) => column.name === columnName)) {
    db.exec(`ALTER TABLE ${tableName} ADD COLUMN ${columnName} ${columnDef}`);
  }
}

ensureColumn('users', 'username', "TEXT DEFAULT ''");
ensureColumn('users', 'first_name', "TEXT DEFAULT ''");
ensureColumn('users', 'subscription_until', 'INTEGER DEFAULT 0');
ensureColumn('users', 'is_premium', 'INTEGER DEFAULT 0');
ensureColumn('users', 'trial_used', 'INTEGER DEFAULT 0');
ensureColumn('users', 'notify_deleted', 'INTEGER DEFAULT 1');
ensureColumn('users', 'notify_edited', 'INTEGER DEFAULT 1');
ensureColumn('users', 'referred_by', 'INTEGER DEFAULT NULL');
ensureColumn('users', 'referrals_count', 'INTEGER DEFAULT 0');
ensureColumn('users', 'is_banned', 'INTEGER DEFAULT 0');
ensureColumn('users', 'ban_reason', "TEXT DEFAULT ''");
ensureColumn('users', 'mtproto_session', "TEXT DEFAULT ''");
ensureColumn('users', 'mtproto_phone', "TEXT DEFAULT ''");
ensureColumn('users', 'created_at', 'INTEGER DEFAULT 0');

ensureColumn('business_connections', 'user_id', 'INTEGER');
ensureColumn('business_connections', 'user_chat_id', 'INTEGER');
ensureColumn('business_connections', 'can_reply', 'INTEGER DEFAULT 0');
ensureColumn('business_connections', 'is_enabled', 'INTEGER DEFAULT 1');
ensureColumn('business_connections', 'updated_at', 'INTEGER DEFAULT 0');

ensureColumn('messages', 'connection_id', "TEXT DEFAULT ''");
ensureColumn('messages', 'owner_id', 'INTEGER');
ensureColumn('messages', 'sender_id', 'INTEGER');
ensureColumn('messages', 'sender_name', "TEXT DEFAULT ''");
ensureColumn('messages', 'sender_username', "TEXT DEFAULT ''");
ensureColumn('messages', 'peer_name', "TEXT DEFAULT ''");
ensureColumn('messages', 'peer_username', "TEXT DEFAULT ''");
ensureColumn('messages', 'text', "TEXT DEFAULT ''");
ensureColumn('messages', 'edit_history', "TEXT DEFAULT '[]'");
ensureColumn('messages', 'reactions_history', "TEXT DEFAULT '[]'");
ensureColumn('messages', 'gift_json', "TEXT DEFAULT ''");
ensureColumn('messages', 'media_type', 'TEXT DEFAULT NULL');
ensureColumn('messages', 'file_id', 'TEXT DEFAULT NULL');
ensureColumn('messages', 'file_name', 'TEXT DEFAULT NULL');
ensureColumn('messages', 'local_path', 'TEXT DEFAULT NULL');
ensureColumn('messages', 'is_self_destruct', 'INTEGER DEFAULT 0');
ensureColumn('messages', 'is_deleted', 'INTEGER DEFAULT 0');
ensureColumn('messages', 'date', 'INTEGER DEFAULT 0');

db.exec(`
  CREATE INDEX IF NOT EXISTS idx_messages_owner_chat
    ON messages(owner_id, chat_id, date);
  CREATE INDEX IF NOT EXISTS idx_messages_chat_date
    ON messages(chat_id, date);
  CREATE INDEX IF NOT EXISTS idx_bconn_user
    ON business_connections(user_id, is_enabled);
`);

const muteCache = new Set();
const bypassCache = new Set();

for (const row of db.prepare('SELECT owner_id, chat_id FROM muted_chats').all()) {
  muteCache.add(`${row.owner_id}_${row.chat_id}`);
}

for (const row of db.prepare('SELECT owner_id, chat_id FROM bypass_chats').all()) {
  bypassCache.add(`${row.owner_id}_${row.chat_id}`);
}

function normalizeUserId(userId) {
  const id = Number(userId);
  if (!Number.isSafeInteger(id) || id <= 0) {
    throw new TypeError('Некорректный userId');
  }
  return id;
}

function nullableInteger(value) {
  if (value === null || value === undefined || value === '') return null;
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

function toFlag(value, defaultValue = 0) {
  if (value === undefined || value === null) return defaultValue;
  if (typeof value === 'boolean') return value ? 1 : 0;
  return Number(value) === 1 ? 1 : 0;
}

function saveBusinessConnection(userId, connectionId, isEnabled) {
  const uid = Number(userId);
  const now = Math.floor(Date.now() / 1000);

  return db
    .prepare(`
      INSERT INTO business_connections (connection_id, user_id, user_chat_id, is_enabled, updated_at)
      VALUES (?, ?, ?, ?, ?)
      ON CONFLICT(connection_id) DO UPDATE SET
        is_enabled = excluded.is_enabled,
        updated_at = excluded.updated_at
    `)
    .run(connectionId, uid, uid, Number(isEnabled), now);
}

function getBusinessConnectionByOwner(userId) {
  const uid = Number(userId);

  return db
    .prepare(
      'SELECT * FROM business_connections WHERE user_id = ? AND is_enabled = 1 LIMIT 1'
    )
    .get(uid);
}

function getUser(userId) {
  const id = Number(userId);
  if (!Number.isSafeInteger(id) || id <= 0) return undefined;

  return db.prepare('SELECT * FROM users WHERE user_id = ?').get(id);
}

function createUser(data) {
  const uid = normalizeUserId(data.userId);
  const now = Math.floor(Date.now() / 1000);

  const existing = getUser(uid);
  if (existing) return { user: existing, isNew: false, referrerId: null };

  const refId = nullableInteger(data.referredBy);
  let finalReferrer = null;

  if (refId && refId !== uid) {
    const referrer = getUser(refId);
    if (referrer && Number(referrer.is_banned) !== 1) {
      finalReferrer = refId;
    }
  }

  db.transaction(() => {
    db.prepare(`
      INSERT INTO users (user_id, username, first_name, subscription_until, trial_used, referred_by, created_at)
      VALUES (?, ?, ?, ?, 0, ?, ?)
    `).run(
      uid,
      data.username || '',
      data.firstName || '',
      now + REGISTRATION_BONUS_DAYS * 86400,
      finalReferrer,
      now
    );

    if (finalReferrer) {
      db.prepare(`
        UPDATE users
        SET referrals_count = referrals_count + 1,
            subscription_until = MAX(subscription_until, ?) + ?
        WHERE user_id = ?
      `).run(now, REFERRAL_BONUS_DAYS * 86400, finalReferrer);
    }
  })();

  return { user: getUser(uid), isNew: true, referrerId: finalReferrer };
}

function addSubscriptionDays(userId, days, isPremium = false) {
  const uid = normalizeUserId(userId);
  const now = Math.floor(Date.now() / 1000);
  const secondsToAdd = Number(days) * 86400;

  const user = getUser(uid);
  if (!user) return false;

  const currentUntil = Number(user.subscription_until || 0);
  const baseTime = currentUntil > now ? currentUntil : now;
  const newUntil = baseTime + secondsToAdd;

  return (
    db
      .prepare(
        'UPDATE users SET subscription_until = ?, is_premium = ? WHERE user_id = ?'
      )
      .run(newUntil, isPremium ? 1 : Number(user.is_premium), uid).changes > 0
  );
}

function setPremium(userId, isPremium) {
  const uid = normalizeUserId(userId);
  const flag = toFlag(isPremium);
  const now = Math.floor(Date.now() / 1000);

  db.transaction(() => {
    if (flag === 1) {
      const user = getUser(uid);
      const currentUntil = Number(user?.subscription_until || 0);
      const baseTime = currentUntil > now ? currentUntil : now;
      const newUntil = baseTime + 365 * 86400;

      db.prepare(
        'UPDATE users SET is_premium = 1, subscription_until = ? WHERE user_id = ?'
      ).run(newUntil, uid);
    } else {
      db.prepare('UPDATE users SET is_premium = 0 WHERE user_id = ?').run(uid);
    }
  })();

  return true;
}

function setBan(userId, isBanned) {
  const uid = normalizeUserId(userId);
  const flag = toFlag(isBanned);

  return (
    db.prepare('UPDATE users SET is_banned = ? WHERE user_id = ?').run(flag, uid)
      .changes > 0
  );
}

function resetSubscription(userId) {
  const uid = normalizeUserId(userId);

  return (
    db
      .prepare(
        'UPDATE users SET subscription_until = 0, is_premium = 0 WHERE user_id = ?'
      )
      .run(uid).changes > 0
  );
}

function setNotifyDeleted(userId, value) {
  const uid = normalizeUserId(userId);

  return (
    db
      .prepare('UPDATE users SET notify_deleted = ? WHERE user_id = ?')
      .run(toFlag(value), uid).changes > 0
  );
}

function setNotifyEdited(userId, value) {
  const uid = normalizeUserId(userId);

  return (
    db
      .prepare('UPDATE users SET notify_edited = ? WHERE user_id = ?')
      .run(toFlag(value), uid).changes > 0
  );
}

function setTrialUsed(userId, value) {
  const uid = normalizeUserId(userId);

  return (
    db
      .prepare('UPDATE users SET trial_used = ? WHERE user_id = ?')
      .run(toFlag(value), uid).changes > 0
  );
}

function hasPremium(userId, adminIds = []) {
  const uid = Number(userId);

  if (Array.isArray(adminIds) && adminIds.map(Number).includes(uid)) return true;

  const user = getUser(uid);
  return Boolean(user && Number(user.is_premium) === 1);
}

function getStats() {
  const now = Math.floor(Date.now() / 1000);

  const totalUsers = db.prepare('SELECT COUNT() as count FROM users').get().count;

  const activeSubs = db
    .prepare(
      'SELECT COUNT() as count FROM users WHERE subscription_until > ? AND is_banned = 0'
    )
    .get(now).count;

  const premiumUsers = db
    .prepare(
      'SELECT COUNT() as count FROM users WHERE is_premium = 1 AND is_banned = 0'
    )
    .get().count;

  const bannedUsers = db
    .prepare('SELECT COUNT() as count FROM users WHERE is_banned = 1')
    .get().count;

  const activeConnections = db
    .prepare('SELECT COUNT(*) as count FROM business_connections WHERE is_enabled = 1')
    .get().count;

  return {
    totalUsers,
    activeSubs,
    premiumUsers,
    bannedUsers,
    activeConnections
  };
}

function getAllUsers() {
  return db.prepare('SELECT user_id as id, * FROM users').all();
}

function getOwnerChats(userId) {
  const uid = Number(userId);

  return db
    .prepare(
      'SELECT chat_id as chatId, COUNT(*) as msgCount FROM messages WHERE owner_id = ? GROUP BY chat_id'
    )
    .all(uid);
}

function getChatMessages(chatId, ownerId) {
  const cid = Number(chatId);
  const oid = Number(ownerId);

  const rows = db
    .prepare(
      'SELECT * FROM messages WHERE chat_id = ? AND owner_id = ? ORDER BY date ASC'
    )
    .all(cid, oid);

  return rows.map(decryptMsgRow);
}

module.exports = {
  getUser,
  createUser,
  saveBusinessConnection,
  getBusinessConnectionByOwner,
  addSubscriptionDays,
  setPremium,
  setBan,
  resetSubscription,
  setNotifyDeleted,
  setNotifyEdited,
  setTrialUsed,
  hasPremium,
  getStats,
  getAllUsers,
  getOwnerChats,
  getChatMessages,
  encryptStr,
  decryptStr
};
