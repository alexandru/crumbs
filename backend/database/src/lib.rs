//! Persistent storage for Crumbs.
//!
//! [`Database`] owns a small SQLite pool, applies the embedded migrations
//! before exposing a usable handle, and is passed to the server at startup.
//! Dropping the handle initiates cleanup; [`Database::close`] waits for it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use tokio::runtime::{Handle, TryCurrentError};

mod cli;
mod config;
mod foods;
mod import;
mod usda_import;

pub use cli::{DbArgs, DbCommand};
pub use config::{DatabaseConfig, DatabasePath};
pub use import::{ImportError, ImportReport, ImportedFoods};

/// Handle to the application database.
///
/// Keeps the database open until it is closed or dropped.
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
    /// Returns an error if no async runtime is active, the parent directory
    /// cannot be created, SQLite cannot open the file, or an embedded migration
    /// cannot be applied or validated.
    pub async fn open(config: DatabaseConfig) -> Result<Self, Error> {
        Handle::try_current().map_err(Error::RuntimeContext)?;
        let path = config.path.clone();
        let pool = open_pool(config).await?;
        if let Err(source) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(Error::Migration { path, source });
        }
        Ok(Self { pool })
    }

    /// Imports a USDA FoodData Central JSON download from `path`.
    ///
    /// Foods commit independently as they are streamed. A later parse or write
    /// failure leaves earlier commits in place.
    ///
    /// # Errors
    ///
    /// Returns an error when the input cannot be opened, parsed, or persisted,
    /// including the number of foods committed before the failure.
    pub async fn import_usda(&self, path: impl AsRef<Path>) -> Result<ImportReport, ImportError> {
        import::import_usda(&self.pool, path.as_ref()).await
    }

    /// Waits for connections to close and prevents further database operations.
    ///
    /// Repeated and concurrent calls are safe. Dropping the handle afterwards
    /// does not repeat the shutdown.
    pub async fn close(&self) {
        self.pool.close().await;
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
    /// Database initialization was attempted outside an async runtime.
    #[error("database initialization requires an async runtime: {0}")]
    RuntimeContext(#[source] TryCurrentError),
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

async fn open_pool(config: DatabaseConfig) -> Result<SqlitePool, Error> {
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
        // FULL synchronizes WAL commits for power-loss durability.
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

    Ok(pool)
}

// SQLite has one writer; the second connection permits a concurrent reader.
const MAX_CONNECTIONS: u32 = 2;
const MIN_CONNECTIONS: u32 = 0;
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
// Two 2 MiB page caches suggest 4 MiB total, not a ceiling on process RSS.
const PAGE_CACHE_KIB: &str = "-2048";
// A checkpoint trigger, not a hard limit on WAL size or checkpoint work.
const WAL_AUTOCHECKPOINT_PAGES: &str = "1000";
const STATEMENT_CACHE_CAPACITY: usize = 32;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
