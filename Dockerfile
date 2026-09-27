# ── Этап сборки: Rust ──
# rust:1-bookworm включает gcc — нужен для rusqlite (bundled sqlite).
FROM rust:1-bookworm AS builder

WORKDIR /build

# Сначала манифесты — чтобы слой кэша зависимостей не сбивался правкой кода.
COPY Cargo.toml Cargo.lock ./
# Заглушка main.rs для прогона `cargo fetch`/первой сборки зависимостей.
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# Код приложения (миграции включаются через include_str! из src/).
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# ── Этап рантайма: только бинарник + статика + CA ──
FROM debian:bookworm-slim

# python3 + pip — для локального TTS/STT (scripts/local_*.py); piper-tts даёт
# офлайн-синтез, faster-whisper — распознавание речи (модель качается отдельно).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl python3 python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && pip3 install --no-cache-dir --break-system-packages piper-tts faster-whisper numpy \
    && useradd --system --create-home --shell /usr/sbin/nologin arena

WORKDIR /app

ENV RUST_LOG=info \
    HOST=0.0.0.0 \
    PORT=3001

COPY --from=builder /build/target/release/negotiation-arena /app/negotiation-arena

# SPA раздаётся из dist/ рядом с бинарником; /static — иконки и пр.
COPY dist ./dist
COPY static ./static

# Локальные TTS/STT: скрипты ищутся от cwd (/app) как scripts/<name>.py.
COPY scripts/local_tts.py scripts/local_stt.py scripts/requirements-voice.txt ./scripts/

# Русский голос Piper для офлайн-TTS: храним в /app/default-voice (вне volume),
# entrypoint при старте сидит его в /app/data/models/piper/ → ключ в админке:
# `piper/ru_RU-ruslan-medium.onnx`. PIPER_VOICE_DOWNLOAD=0 — без голоса.
ARG PIPER_VOICE_DOWNLOAD=1
ARG PIPER_RU_VOICE_URL=https://huggingface.co/rhasspy/piper-voices/resolve/main/ru/ru_RU/ruslan/medium/ru_RU-ruslan-medium.onnx
RUN if [ "$PIPER_VOICE_DOWNLOAD" = "1" ]; then \
        mkdir -p /app/default-voice \
        && curl -fsSL "$PIPER_RU_VOICE_URL" -o /app/default-voice/ru_RU-ruslan-medium.onnx \
        && curl -fsSL "$PIPER_RU_VOICE_URL.json" -o /app/default-voice/ru_RU-ruslan-medium.onnx.json; \
    fi

# Entry-point: сидит голос в volume, затем запускает сервер.
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

# DB и локальные модели переживают рестарты через volume.
RUN mkdir -p /app/data /app/data/models \
    && chown -R arena:arena /app

USER arena

EXPOSE 3001
VOLUME ["/app/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" || exit 1

CMD ["/app/negotiation-arena"]
ENTRYPOINT ["/app/docker-entrypoint.sh"]
