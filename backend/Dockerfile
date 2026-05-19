FROM rust:1-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home --home-dir /app app
WORKDIR /app
COPY --from=builder /app/target/release/podcast_backend /usr/local/bin/podcast_backend
RUN mkdir -p /app/data /app/resurs && chown -R app:app /app

USER app
EXPOSE 8787

ENV PODCAST_BACKEND_ADDR=0.0.0.0:8787
ENV PODCAST_BACKEND_DATA=/app/data/app_state.json
ENV PODCAST_BACKEND_RESOURCES=/app/resurs
ENV PODCAST_PUBLIC_BASE_URL=http://127.0.0.1:8787

CMD ["podcast_backend"]
