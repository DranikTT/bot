#!/usr/bin/env bash
# Оффлайн-тесты на локальном двойнике Bot API (сеть не нужна).
set -euo pipefail
cd "$(dirname "$0")"

export LC_ALL=${LC_ALL:-C.UTF-8}
export LANG=${LANG:-C.UTF-8}
JAVA=${JAVA:-java}
CP="build/classes:build/test-classes:lib/json.jar:lib/sqlite-jdbc-3.49.1.0.jar"

[ -d build/test-classes ] || ./build.sh

BOT_TOKEN=${BOT_TOKEN:-test:token} \
ADMIN_IDS=${ADMIN_IDS:-777} \
"$JAVA" -Dfile.encoding=UTF-8 -cp "$CP" bot.FakeTelegramTest
