//! Persistent storage for Crumbs.
//!
//! [`Database`] owns a small SQLite pool, applies the embedded migrations
//! before exposing a usable handle, and is passed to the server at startup.
//! Dropping the handle blocks until SQLx has returned and closed its connections.
//!
//! # Resource settings
//!
//! Two connections are allowed, and each suggests a 2 MiB SQLite page cache
//! (`cache_size = -2048`), so the suggested page caches total at most about
//! 4 MiB. This is not a process RSS ceiling: SQLite, SQLx, the allocator, and
//! other parts of the process use memory outside the page cache, and
//! `mmap_size = 0` disables memory-mapped I/O. The pool keeps no minimum number
//! of connections warm and closes idle connections after 60 seconds. SQLite
//! permits one writer regardless of pool size, so the second connection can
//! serve a concurrent reader rather than parallel writes. The 1000-page WAL
//! threshold triggers an automatic checkpoint; it neither bounds checkpoint
//! work nor hard-bounds WAL growth. Synchronous mode remains `FULL` for durable
//! commits.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use tokio::runtime::{Handle, RuntimeFlavor};

mod config;

pub use config::{DatabaseConfig, DatabasePath};

/// Handle to the application database.
///
/// The handle owns its connection pool and waits for connection shutdown when
/// dropped. Its originating multi-thread Tokio runtime must outlive it so SQLx
/// can finish pending connection returns. Database operations run on that runtime.
/// Connections stay private to this handle; operations must release them before
/// the owner can be dropped.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens the database at the configured path and applies pending migrations.
    ///
    /// Missing parent directories and the database file are created.
    /// Opening requires an active multi-thread Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns an error when the runtime is missing or uses the current-thread
    /// scheduler, the parent directory cannot be created, SQLite
    /// cannot open the file, or an embedded migration cannot be applied or
    /// validated. No handle is returned on failure.
    pub async fn open(config: DatabaseConfig) -> Result<Self, Error> {
        let runtime = Handle::try_current().map_err(|_| Error::RuntimeContext)?;
        if runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
            return Err(Error::RuntimeContext);
        }

        if let Some(parent) = config
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| Error::CreateDirectory {
                    path: parent.to_path_buf(),
                    source,
                })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(&config.path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            // SQLite FULL synchronizes commits to disk; this is independent of Rust async execution.
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT)
            .pragma("cache_size", PAGE_CACHE_KIB)
            .pragma("mmap_size", "0")
            .pragma("wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)
            .statement_cache_capacity(STATEMENT_CACHE_CAPACITY);

        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .min_connections(MIN_CONNECTIONS)
            .idle_timeout(IDLE_TIMEOUT)
            .connect_with(options)
            .await
            .map_err(|source| Error::Open {
                path: config.path.clone(),
                source,
            })?;

        let database = Self { pool };
        MIGRATOR
            .run(&database.pool)
            .await
            .map_err(|source| Error::Migration {
                path: config.path,
                source,
            })?;

        Ok(database)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        if let Ok(runtime) = Handle::try_current()
            && runtime.runtime_flavor() == RuntimeFlavor::MultiThread
        {
            // Let other Tokio tasks return connections while this thread waits.
            tokio::task::block_in_place(|| runtime.block_on(self.pool.close()));
        } else {
            // SQLite shutdown uses worker channels, so it can also be driven
            // outside Tokio without nesting another Tokio runtime.
            futures_lite::future::block_on(self.pool.close());
        }
    }
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

/// Errors produced while opening or preparing the database.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Opening requires an active multi-thread Tokio runtime.
    #[error("opening the database requires a multi-thread Tokio runtime")]
    RuntimeContext,
    /// A parent directory for the database file could not be created.
    #[error("failed to create the database directory {}: {source}", path.display())]
    CreateDirectory {
        /// Directory that could not be created.
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// SQLite could not open the database file.
    #[error("failed to open the database {}: {source}", path.display())]
    Open {
        /// Database file that could not be opened.
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },
    /// An embedded migration could not be applied or validated.
    #[error("failed to apply database migrations to {}: {source}", path.display())]
    Migration {
        /// Database file whose migrations failed.
        path: PathBuf,
        #[source]
        source: MigrateError,
    },
}

const MAX_CONNECTIONS: u32 = 2;
const MIN_CONNECTIONS: u32 = 0;
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const PAGE_CACHE_KIB: &str = "-2048";
const WAL_AUTOCHECKPOINT_PAGES: &str = "1000";
const STATEMENT_CACHE_CAPACITY: usize = 32;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
