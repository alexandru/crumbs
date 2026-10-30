//! Persistent storage for Crumbs.
//!
//! [`Database`] owns a small SQLite pool, applies the embedded migrations
//! before exposing a usable handle, and is passed to the server at startup.
//! Dropping the handle blocks until SQLx has returned and closed its connections.

use std::fmt;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use tokio::runtime::Handle;
use tokio::sync::oneshot;

mod config;

pub use config::{DatabaseConfig, DatabasePath};

/// Handle to the application database.
///
/// Owns its connection pool and execution resources. Dropping the handle waits
/// for connection shutdown.
pub struct Database {
    pool: SqlitePool,
    runtime: Handle,
    worker: Worker,
}

impl Database {
    /// Opens the database at the configured path and applies pending migrations.
    ///
    /// Missing parent directories and the database file are created.
    ///
    /// # Errors
    ///
    /// Returns an error if execution resources cannot be created, the parent
    /// directory cannot be created, SQLite cannot open the file, or an embedded
    /// migration cannot be applied or validated.
    pub async fn open(config: DatabaseConfig) -> Result<Self, Error> {
        let path = config.path.clone();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("crumbs-database".into())
            .spawn(move || run_worker(config, ready_sender, shutdown_receiver))
            .map_err(Error::Worker)?;
        let worker = Worker {
            shutdown: Some(shutdown_sender),
            thread: Some(thread),
        };
        let (pool, runtime) = ready_receiver.await.map_err(Error::WorkerStopped)??;
        let database = Self {
            pool,
            runtime,
            worker,
        };
        let pool = database.pool.clone();
        database
            .run(async move {
                MIGRATOR
                    .run(&pool)
                    .await
                    .map_err(|source| Error::Migration { path, source })
            })
            .await??;
        Ok(database)
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: impl Future<Output = T> + Send + 'static,
    ) -> Result<T, Error> {
        // Connection acquisition, returns, and cancellation stay on the owned runtime.
        let mut task = Operation(self.runtime.spawn(operation));
        (&mut task.0).await.map_err(Error::Operation)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let pool = self.pool.clone();
        let close = self.runtime.spawn(async move { pool.close().await });
        let _ = futures_lite::future::block_on(close);
        self.worker.shutdown.take();
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
    /// The database worker thread could not be created.
    #[error("failed to start the database worker: {0}")]
    Worker(#[source] io::Error),
    /// The database runtime could not be created.
    #[error("failed to create the database runtime: {0}")]
    Runtime(#[source] io::Error),
    /// The database worker stopped before reporting its startup result.
    #[error("database worker stopped during startup: {0}")]
    WorkerStopped(#[source] oneshot::error::RecvError),
    /// A database operation stopped before completion.
    #[error("database operation stopped before completion: {0}")]
    Operation(#[source] tokio::task::JoinError),
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

struct Worker {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

struct Operation<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for Operation<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the channel also covers cancellation while open awaits readiness.
        self.shutdown.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_worker(
    config: DatabaseConfig,
    ready: oneshot::Sender<Result<(SqlitePool, Handle), Error>>,
    shutdown: oneshot::Receiver<()>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(Error::Runtime(error)));
            return;
        }
    };
    runtime.block_on(async move {
        match open_pool(config).await {
            Ok(pool) => {
                if ready.send(Ok((pool.clone(), Handle::current()))).is_ok() {
                    let _ = shutdown.await;
                }
                pool.close().await;
            }
            Err(error) => {
                let _ = ready.send(Err(error));
            }
        }
    });
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
#[path = "../unit-tests/lib.rs"]
mod tests;
