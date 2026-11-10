use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, SystemTime};

use crate::clock::Clock;
use crate::{Database, DatabaseConfig};

mod energy;
mod model;
mod sqlite;

pub(super) async fn open_database(
    initial_time_ms: u64,
) -> Result<(tempfile::TempDir, Database, Arc<AtomicU64>), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let opened = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
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
