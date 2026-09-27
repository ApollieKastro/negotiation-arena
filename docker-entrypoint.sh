#!/bin/sh
# Сидим офлайн-голос Piper в volume перед стартом сервера.
#
# Образ кладёт голос в /app/default-voice (вне volume), а volume /app/data
# переживает рестарты и уже существует у действующих деплоев — поэтому при
# первом запуске копируем голос внутрь models/piper/, если его там нет.
set -e

DEFAULT_VOICE_DIR="/app/default-voice"
MODELS_PIPER_DIR="/app/data/models/piper"

if [ -d "$DEFAULT_VOICE_DIR" ]; then
    mkdir -p "$MODELS_PIPER_DIR"
    for f in "$DEFAULT_VOICE_DIR"/*; do
        [ -f "$f" ] || continue
        base="$(basename "$f")"
        if [ ! -e "$MODELS_PIPER_DIR/$base" ]; then
            cp "$f" "$MODELS_PIPER_DIR/$base"
        fi
    done
fi

# CMD передаётся в "$@" — так entrypoint остаётся совместимым с любым
# переопределением команды (например, `docker run … sh`).
exec "$@"
