# ── Stage 1: сборка ──────────────────────────────────────────────
FROM rust:slim AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev perl make \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .
RUN cargo build --release

# ── Stage 2: runtime ─────────────────────────────────────────────
FROM debian:bookworm-slim

# ca-certificates нужен для TLS (rustls проверяет системные корневые сертификаты)
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/*

# Бинарник кладём ВНЕ /app, потому что /app монтируется исходниками при запуске
COPY --from=builder /src/target/release/alyaska_garant /usr/local/bin/alyaska_garant

# Ассеты (баннеры, аватарки) — тоже вне /app, бот обращается к ним по
# относительному пути "assets/...", поэтому рабочая директория = /bot
WORKDIR /bot
COPY --from=builder /src/assets /bot/assets

# Каталог для персистентных данных (SQLite)
RUN mkdir -p /app/data

# DATABASE_URL указывает SQLite хранить файл в персистентном /app/data
ENV DATABASE_URL="sqlite:/app/data/data.db"

CMD ["/usr/local/bin/alyaska_garant"]
