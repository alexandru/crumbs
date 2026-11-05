# Backend composition and configuration ownership

The Rust workspace lives entirely under `backend/` rather than the repository root, leaving room for non-Rust applications such as a frontend or Android client. `backend/main` is the executable and composition root; it depends on `backend/server`, `backend/database`, and `backend/import`. `server` and `import` depend on `database`. This deliberately favors independent library composition over a monolithic binary or a root-level Rust workspace.

`main` owns command-line parsing and environment-variable settings. It composes and passes typed `DatabaseConfig` and `HttpConfig` values into the libraries that own them. CLI arguments override environment variables, which override defaults. Each command accepts only the settings it needs: database operations do not require HTTP settings or an OAuth issuer. This keeps dependencies acyclic and leaves each library's configuration owned by the functionality it configures.

The executable owns the `db import-usda` command and composes the database with the import library. The database owns source-independent food persistence and does not depend on source importers or command-line parsing.
