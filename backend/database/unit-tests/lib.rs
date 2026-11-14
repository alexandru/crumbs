use std::error::Error as StdError;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, SystemTime};

use crate::clock::Clock;
use crate::identity::UserId;
use crate::{Database, DatabaseConfig};

pub(crate) async fn open_database() -> Result<(tempfile::TempDir, Database), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;

    let database = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;

    Ok((directory, database))
}

pub(crate) async fn open_database_with_clock(
    initial_time_ms: u64,
) -> Result<(tempfile::TempDir, Database, Arc<AtomicU64>), Box<dyn StdError>> {
    let (directory, opened) = open_database().await?;

    let clock_value = Arc::new(AtomicU64::new(initial_time_ms));
    let clock_source = Arc::clone(&clock_value);
    let database = Database {
        pool: opened.pool.clone(),
        clock: Clock::with_source(move || {
            SystemTime::UNIX_EPOCH
                + Duration::from_millis(clock_source.load(std::sync::atomic::Ordering::SeqCst))
        }),
    };

    drop(opened);

    Ok((directory, database, clock_value))
}

pub(crate) async fn insert_user(database: &Database, id: i64) -> Result<UserId, Box<dyn StdError>> {
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, active, created_at)
         VALUES (?, ?, 'hash', 1, 1)",
    )
    .bind(id)
    .bind(format!("user{id}"))
    .execute(&database.pool)
    .await?;

    Ok(UserId::new(id)?)
}
