FROM rust:1.78-alpine3.18 as builder

ENV RUSTFLAGS="-C target-feature=-crt-static"

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

COPY . /tmp/rust/src/github.com/soulgarden/logalert

VOLUME ./target /tmp/rust/src/github.com/soulgarden/logalert

WORKDIR /tmp/rust/src/github.com/soulgarden/logalert

RUN cargo build --target=x86_64-unknown-linux-musl --release

FROM alpine:3.20

RUN apk add --no-cache libgcc

RUN adduser -S www-data -G www-data

COPY --from=builder --chown=www-data /tmp/rust/src/github.com/soulgarden/logalert/target/x86_64-unknown-linux-musl/release/logalert /bin/logalert

RUN chmod +x /bin/logalert

USER www-data

CMD ["/bin/logalert"]
