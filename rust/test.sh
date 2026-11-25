#!/usr/bin/env bash
# Все тесты: юнит + интеграционные на фейковом Telegram.
set -euo pipefail

cd "$(dirname "$0")"
exec cargo test "$@"
