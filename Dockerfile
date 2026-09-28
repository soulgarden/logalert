# syntax=docker/dockerfile:1

FROM rust:1.98.1-alpine3.24 AS builder

RUN apk add --no-cache cmake make musl-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src/ ./src/

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --locked && \
    install -Dm755 target/release/logalert /out/logalert

FROM alpine:3.24.2

RUN apk add --no-cache ca-certificates && \
    adduser -S -u 10001 -G www-data www-data

COPY --from=builder /out/logalert /usr/local/bin/logalert

ENV CFG_PATH=/config.json
USER 10001:82
CMD ["/usr/local/bin/logalert"]
