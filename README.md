# Crumbs

**Everyday nutrition, without the noise.**

Crumbs is being built for self-hosted daily calorie tracking: what you eat, what you burn, and the balance between them. The first milestone is a common-food catalog that an MCP server can look up and extend.

**Early development** · runnable Rust backend scaffold.

## Quick start

You need the latest stable Rust toolchain; Clippy and rustfmt are configured.

From the repository root:

```sh
cd backend
cargo run -p crumbs-main -- server start
```

Expect `Hello, world!`; the process keeps running until Ctrl+C or another shutdown signal.

## Container

From the repository root:

```sh
docker build -t crumbs ./backend
docker run --rm --name crumbs crumbs
```

The image runs as a non-root user and exposes no ports. Publishing to GHCR is manual via the [publish workflow](.github/workflows/publish.yml).

## Development

From the repository root:

```sh
cd backend
cargo xtask check
```

This checks formatting, runs Clippy, tests, and builds the workspace with warnings as errors. The CLI lives in `backend/main`, server lifecycle in `backend/server`, storage in `backend/database`, and workspace automation in `backend/xtask`.
