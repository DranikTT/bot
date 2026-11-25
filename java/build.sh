#!/usr/bin/env bash
# Сборка бота. Нужен только JDK 11+ и jar-файлы из java/lib.
set -euo pipefail

cd "$(dirname "$0")"

# UTF-8 обязателен: иначе русский текст в аргументах и логах превращается в «?»
export LC_ALL=${LC_ALL:-C.UTF-8}
export LANG=${LANG:-C.UTF-8}

JAVAC=${JAVAC:-javac}
CP="lib/json.jar:lib/sqlite-jdbc-3.49.1.0.jar"

echo "==> Компилирую исходники"
rm -rf build/classes build/test-classes
mkdir -p build/classes build/test-classes

$JAVAC -encoding UTF-8 -d build/classes -cp "$CP" $(find src/main -name "*.java")

if [ -d src/test ]; then
  echo "==> Компилирую тесты"
  $JAVAC -encoding UTF-8 -d build/test-classes -cp "build/classes:$CP" $(find src/test -name "*.java")
fi

echo "==> Готово: build/classes"
