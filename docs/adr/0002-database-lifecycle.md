# Database lifecycle

`Database` owns its `SqlitePool`; normal handle destruction drops the pool and initiates nonblocking connection cleanup, which can complete in the background. Calling `close().await` is optional and waits for connections to close, including checked-out connections; forgetting it does not leak connections when the handle is normally dropped. This keeps resource ownership explicit without blocking destructors, private runtimes, or custom shutdown machinery.

Changes to this lifecycle decision require agreement with the repository owner.

The SQLite cleanup tests in `backend/database/unit-tests/runtime.rs`, particularly `dropping_database_is_nonblocking_and_pool_eventually_closes` and `close_waits_for_checked_out_connections_and_is_idempotent`, verify the drop and explicit-close contracts against actual SQLite file handles on Linux.
