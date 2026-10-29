//! Persistent storage for Crumbs.
//!
//! [`Database`] owns a small SQLite pool, applies the embedded migrations
//! before exposing a usable handle, and is passed to the server at startup.
//! Dropping the handle closes the pool immediately and asks SQLite workers to
//! close. Worker shutdown is asynchronous and is not awaited by `Drop`.
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

mod config;

pub use config::{DatabaseConfig, DatabasePath};

/// Handle to the application database.
///
/// The handle owns its connection pool. Dropping it closes the pool and starts
/// best-effort asynchronous cleanup when a Tokio runtime is available.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens the database at the configured path and applies pending migrations.
    ///
    /// Missing parent directories and the database file are created.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent directory cannot be created, SQLite
    /// cannot open the file, or an embedded migration cannot be applied or
    /// validated. No handle is returned on failure.
    pub async fn open(config: DatabaseConfig) -> Result<Self, Error> {
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
        // Pool::close marks the pool closed before returning its async wait future.
        // Dropping that future skips waiting; schedule the wait when possible.
        drop(self.pool.close());
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let pool = self.pool.clone();
            runtime.spawn(async move { pool.close().await });
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
