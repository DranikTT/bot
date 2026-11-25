#!/usr/bin/env bash
# Сборка релизного бинарника. Внешние крейты не нужны — только std и libcurl.
set -euo pipefail

cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "✖ cargo не найден. Установите Rust: https://rustup.rs" >&2
  exit 1
fi

echo "→ сборка savemod_bot (release)"
cargo build --release

echo "✔ готово: target/release/savemod_bot"
