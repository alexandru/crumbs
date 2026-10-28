# Backend composition and configuration ownership

The Rust workspace lives entirely under `backend/` rather than the repository root, leaving room for non-Rust applications such as a frontend or Android client. `backend/main` is the executable and composition root; it depends on `backend/server` and `backend/database`, while `server` depends on `database`. This deliberately favors independent library composition over a monolithic binary or a root-level Rust workspace.

`AppConfig` belongs to `main` and composes `HttpConfig`, which belongs to `server`. `main` loads TOML with `config-rs` and passes typed subsystem configuration into `server`; `server` does not depend on `main` or an application configuration loader. This keeps dependencies acyclic and leaves each library's configuration owned by the functionality it configures.
