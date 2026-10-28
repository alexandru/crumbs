# Crumbs

Crumbs is a self-hosted calorie tracker in early development. The first milestone is a food catalog that an MCP server can search and extend; the goal is to track daily caloric intake and burn.

## Quick start

Requires stable Rust.

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
