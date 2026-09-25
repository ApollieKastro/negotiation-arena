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

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --shell /usr/sbin/nologin arena

WORKDIR /app

ENV RUST_LOG=info \
    HOST=0.0.0.0 \
    PORT=3001

COPY --from=builder /build/target/release/negotiation-arena /app/negotiation-arena

# SPA раздаётся из dist/ рядом с бинарником; /static — иконки и пр.
COPY dist ./dist
COPY static ./static

# DB и локальные модели переживают рестарты через volume.
RUN mkdir -p /app/data /app/data/models \
    && chown -R arena:arena /app

USER arena

EXPOSE 3001
VOLUME ["/app/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" || exit 1

CMD ["/app/negotiation-arena"]
