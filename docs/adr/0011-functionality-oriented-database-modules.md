---
status: accepted
---

# Organize database code by functionality

Reorganize `backend/database` around functionality modules so that each functionality owns its storage-independent API and its private SQLite implementation in separate files. Each folder exposes a clean interface through `mod.rs`, while implementation submodules remain private.

Reusable definitions stay with their natural owner, and dependencies between modules must be acyclic. Runtime behavior and stored-data semantics remain unchanged.

Crate APIs may change where cleanup improves ownership and clarity. Public database interfaces must not expose SQLx types; workspace callers will be updated to the cleaned interfaces. These API changes must preserve the application's functionality, wire contracts, and stored-data semantics.

The `identity/` module owns users and OAuth, including their contracts and private SQLite implementations in separate files. It owns account and session operations that must remain atomic, including revocation during password changes and deactivation.

The shared nutrition, serving, and measurement data model is named `nutrition_model`. It owns the corresponding types, parsing, validation, serialization, and unit conversions.

The functionality owners are `foods/`, `nutrition_model/`, and `identity/`. Foods owns shared food records, source-supplied food writes and lookup, custom-food management, search, and tags. The database handle, connection pool, configuration, clock, and migration loading remain shared infrastructure.

The `foods/` interface is exported through `mod.rs`, with private `model.rs`, `source_foods.rs`, `custom_foods.rs`, `search.rs`, and `tags.rs` contract/model files. Private SQLite implementations mirror the operation files under `foods/sqlite/`, with shared private row decoding in `rows.rs`.

The `identity/` interface is exported through `mod.rs`, with private `users.rs` and `oauth.rs` contract/model files. Private SQLite implementations live under `identity/sqlite/`; `sessions.rs` contains shared transaction-local revocation operations used by user management and OAuth. Callers provide the active transaction so password changes and session revocation remain atomic.

Contract and model files do not depend on SQLite implementations. SQLite implementations depend on their owner's contracts and shared database infrastructure. Public errors belong with their contracts and expose no SQLx types.

Food operations are exposed through the `FoodWriter`, `FoodLookup`, `FoodSearch`, `CustomFoodStore`, and `TagStore` traits exported by `foods`. `Database` exposes lifecycle operations such as `open` and `close`; domain operations are accessed through their traits. `FoodSearch` replaces the search-only `FoodReader` name.

The shared food record is named `StoredFood` instead of `FoodSearchResult`; grouped search output is named `SearchResults` instead of `FoodSearchResults`; the count used by search and custom-food listing is named `FoodLimit` instead of `SearchLimit`. Workspace callers use the cleaned names. Serialized payloads and limit behavior remain unchanged.

The crate root is a facade exposing the `foods`, `identity`, and `nutrition_model` namespaces plus the shared database handle, configuration, and initialization errors. Shared connection ownership and lifecycle live in `runtime.rs`; configuration and the persistence clock remain in `config.rs` and `clock.rs`. The nutrition model uses private `model.rs` and `quantity.rs` files behind its `mod.rs` interface.

Existing migration files and checksums, schema, serialized payloads, transaction atomicity, and database lifecycle behavior are preserved. Unit tests mirror source ownership and retain the existing behavioral assertions.
