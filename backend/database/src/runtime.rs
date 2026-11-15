use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use crate::clock::Clock;
use crate::config::DatabaseConfig;

pub struct Database {
    pub(crate) pool: SqlitePool,
    pub(crate) clock: Clock,
}

impl Database {
    pub async fn open(config: DatabaseConfig) -> Result<Self, Error> {
        let path = std::path::absolute(&config.path).map_err(|source| Error::ResolvePath {
            path: config.path,
            source,
        })?;
        let pool = open_pool(&path).await?;
        if let Err(source) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(Error::Migration {
                path,
                source: Box::new(source),
            });
        }
        Ok(Self {
            pool,
            clock: Clock::default(),
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to resolve the database path {}: {source}", path.display())]
    ResolvePath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create the database directory {}: {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to open the database {}: {source}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    #[error("failed to apply database migrations to {}: {source}", path.display())]
    Migration {
        path: PathBuf,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

async fn open_pool(path: &Path) -> Result<SqlitePool, Error> {
    if let Some(parent) = path
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
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
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
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;

    Ok(pool)
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
#[path = "../unit-tests/runtime.rs"]
mod tests;
