# Keep the public database API independent of its storage engine

The entire public API of the database crate—including types, signatures, errors, query contracts, and documented semantics—must remain storage-engine-independent to reduce coupling and allow storage changes. `Database`, `FoodWriter`, and `FoodReader` illustrate this boundary; they do not limit its scope. SQLite is the pragmatic current storage engine, not a permanent public dependency, so SQLite-specific types and semantics stay inside its adapter.

The Crumbs full-text expression syntax is modeled on SQLite FTS5 but owned by Crumbs. A future PostgreSQL adapter may parse or translate that syntax; identical ranking or tokenization behavior is not required. No PostgreSQL implementation is added now.
