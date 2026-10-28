# syntax=docker/dockerfile:1

FROM rust:1-slim-trixie AS builder

WORKDIR /build
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release --package crumbs-main --bin crumbs \
    && cp /build/target/release/crumbs /build/crumbs

FROM debian:trixie-slim AS runtime

LABEL org.opencontainers.image.source="https://github.com/alexandru/crumbs"

WORKDIR /app
COPY resources/application-sample.toml /app/application.toml
COPY --from=builder /build/crumbs /usr/local/bin/crumbs

EXPOSE 8080

USER 65532:65532

ENTRYPOINT ["/usr/local/bin/crumbs"]
CMD ["server", "start", "--config", "/app/application.toml"]
STOPSIGNAL SIGTERM
