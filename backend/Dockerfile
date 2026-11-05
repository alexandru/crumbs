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
COPY --from=builder /build/crumbs /usr/local/bin/crumbs
RUN mkdir -p /app/data && chown ${APP_UID}:${APP_GID} /app/data

ENV CRUMBS_DATABASE_PATH=/app/data/crumbs.db \
    CRUMBS_HTTP_BIND=0.0.0.0 \
    CRUMBS_HTTP_PORT=8080

EXPOSE 8080

USER ${APP_UID}:${APP_GID}

ENTRYPOINT ["/usr/local/bin/crumbs"]
CMD ["server", "start"]
STOPSIGNAL SIGTERM
