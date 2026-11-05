# syntax=docker/dockerfile:1

FROM rust:1-slim-trixie AS builder

WORKDIR /build
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release --package crumbs-main --bin crumbs \
    && cp /build/target/release/crumbs /build/crumbs

FROM debian:trixie-slim AS runtime

# Keep arbitrary unprivileged IDs consistent with ownership of the database directory.
ARG APP_UID=65532
ARG APP_GID=65532

LABEL org.opencontainers.image.source="https://github.com/alexandru/crumbs"

WORKDIR /app
COPY resources/application-docker.toml /app/application.toml
COPY --from=builder /build/crumbs /usr/local/bin/crumbs
RUN mkdir -p /app/data && chown ${APP_UID}:${APP_GID} /app/data

EXPOSE 8080

USER ${APP_UID}:${APP_GID}

ENTRYPOINT ["/usr/local/bin/crumbs"]
CMD ["server", "start", "--config", "/app/application.toml"]
STOPSIGNAL SIGTERM
