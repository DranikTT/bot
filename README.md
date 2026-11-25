# Бот перехвата удалённых и изменённых сообщений (Java)

Telegram-бот для работы с **Telegram Business**: сохраняет переписку,
уведомляет об удалённых и изменённых сообщениях, экспортирует чаты в HTML
и импортирует историю из Telegram Desktop.

Версия на **Java 11+** — без Maven, Gradle и сторонних фреймворков.
Только два jar-файла (JSON и SQLite), которые лежат в `java/lib`.

**Кастомные эмодзи возвращены через HTML:** тексты уходят с
`parse_mode=HTML` и тегом `<tg-emoji emoji-id="…">💎</tg-emoji>`,
а кнопки получают то же самое через поле `icon_custom_emoji_id`.
ID взяты из прежней версии проекта, поэтому картинки те же.

## Возможности

- **Меню** — дизайн 2+2+1: `Тарифы` / `Пригласить друга`,
  `Подключение` / `Создать бота`, `Настройки`.
- **Кастомные эмодзи** в тексте и в кнопках, со стилями `primary` / `success` / `danger`.
- **Telegram Business** — сохранение переписки, перехват удалённых
  и изменённых сообщений, сгорающие медиа по точке `.` или запятой `,`.
- **Тарифы и оплата** звёздами: 7 дней — 25 ⭐, 30 — 75 ⭐, 90 — 150 ⭐, Премиум — 300 ⭐.
- **Реферальная система**: приглашённый +3 дня, пригласивший +7 дней.
- **Экспорт** переписки в самодостаточный HTML (медиа внутри base64,
  защита от XSS) и **импорт** `result.json` / ZIP из Telegram Desktop.
- **Админ-панель**: статистика, выдача подписки, премиум, бан, рассылка.
- **Шифрование** текстов в базе: AES-256-GCM, префикс `enc:v1:` —
  совместим с прежними версиями бота.

## Требования

- JDK 11 или новее (`java` и `javac` в PATH).
- Никаких менеджеров зависимостей: `java/lib/json.jar` и
  `java/lib/sqlite-jdbc-3.49.1.0.jar` уже в репозитории.

## Установка

```bash
cp .env.example .env
# впишите BOT_TOKEN и ADMIN_IDS

cd java
./build.sh      # компиляция
./run.sh        # запуск
```

Запуск без скриптов:

```bash
cd java
javac -encoding UTF-8 -d build/classes -cp "lib/json.jar:lib/sqlite-jdbc-3.49.1.0.jar" \
      $(find src/main -name "*.java")
java -Dfile.encoding=UTF-8 -cp "build/classes:lib/json.jar:lib/sqlite-jdbc-3.49.1.0.jar" bot.Bot
```

## Тесты

Оффлайн, сеть не нужна — поднимается локальный двойник Bot API:

```bash
cd java
./test.sh
```

Проверяется: отправка меню с баннером и текстовый фолбэк, `<tg-emoji>`
в HTML, `icon_custom_emoji_id` в кнопках, навигация по всем разделам,
оплата и продление подписки, реферальные бонусы, шифрование и его
совместимость, экспорт HTML с экранированием XSS, импорт `result.json`,
защита от zip-slip, баны и валидация клавиатур.

## Структура

```
java/
├── lib/                      json.jar, sqlite-jdbc-3.49.1.0.jar
├── src/main/java/bot/
│   ├── Bot.java              точка входа, long polling
│   ├── Config.java           .env и переменные окружения
│   ├── TelegramApi.java      тонкий клиент Bot API (java.net.http)
│   ├── Json.java             JSON без зависимостей
│   ├── Emoji.java            кастомные эмодзи: <tg-emoji> и icon_custom_emoji_id
│   ├── Keyboard.java         сборка клавиатур + валидация callback_data
│   ├── Keyboards.java        все меню
│   ├── Menus.java            тексты меню
│   ├── MenuSender.java       отправка с баннером и фолбэком
│   ├── Database.java         SQLite + AES-256-GCM
│   ├── Archive.java          экспорт HTML / импорт Telegram Desktop
│   └── UpdateHandler.java    обработка апдейтов
└── src/test/java/bot/        FakeTelegram + 43 проверки
```

## Переменные окружения

| Переменная | По умолчанию | Назначение |
|---|---|---|
| `BOT_TOKEN` | — | токен от @BotFather, обязателен |
| `ADMIN_IDS` | — | ID админов через запятую |
| `DB_ENCRYPTION_KEY` | встроенный | ключ AES-256-GCM |
| `DATABASE_PATH` | `bot_database.sqlite` | путь к базе |
| `API_SERVER` | `https://api.telegram.org` | адрес Bot API |
| `MAX_EXPORT_MB` | 48 | лимит экспорта |
| `MAX_SINGLE_MEDIA_MB` | 35 | лимит одного медиа |
| `MAX_IMPORT_FILE_MB` | 200 | лимит файла импорта |
| `MAX_IMPORTED_MEDIA_MB` | 100 | лимит медиа при импорте |

## Команды

`/start` — меню · `/menu` — меню · `/export` — архив в HTML ·
`/archive` — импорт из Telegram Desktop · `/connect` — подключение ·
`/admin` — админ-панель.

## Про кастомные эмодзи

Telegram отдаёт кастомные эмодзи в кнопках только ботам, чей владелец
имеет **Telegram Premium** (или купил юзернейм на Fragment). Если прав нет,
бот не падает: `MenuSender` повторяет отправку без иконок в кнопках
(`Keyboard.stripEmojis`), а `<tg-emoji>` в тексте Telegram просто
показывает обычный эмодзи-фолбэк.

## Заметки

- Тексты и подписи отправляются с `parse_mode=HTML`.
- Баннеры лежат в `assets/`, путь можно переопределить (`BANNER_*`).
  Если файла нет или он не картинка — меню уходит текстом, а не падает
  с `DOCUMENT_INVALID`.
- `./run.sh` выставляет `LANG=C.UTF-8`: без UTF-8 локали в контейнере
  русский текст в аргументах и логах превращается в `?`.
