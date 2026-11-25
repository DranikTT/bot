#!/usr/bin/env bash
# Запуск бота. Конфиг читается из ../.env (или переменных окружения).
set -euo pipefail

cd "$(dirname "$0")"

BIN="target/release/savemod_bot"
if [ ! -x "$BIN" ]; then
  echo "→ бинарник не собран, собираю"
  ./build.sh
fi

exec "$BIN" "$@"
