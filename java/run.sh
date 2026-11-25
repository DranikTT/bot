#!/usr/bin/env bash
# Запуск бота. Конфигурация читается из .env в корне проекта.
set -euo pipefail
cd "$(dirname "$0")"

export LC_ALL=${LC_ALL:-C.UTF-8}
export LANG=${LANG:-C.UTF-8}
JAVA=${JAVA:-java}
CP="build/classes:lib/json.jar:lib/sqlite-jdbc-3.49.1.0.jar"

if [ ! -d build/classes ]; then
  ./build.sh
fi

exec "$JAVA" -Dfile.encoding=UTF-8 -cp "$CP" bot.Bot "$@"
