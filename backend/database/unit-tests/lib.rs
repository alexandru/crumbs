use std::error::Error as StdError;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use sqlx::{SqliteConnection, query, query_scalar};

use super::{Database, DatabaseConfig, Error};

#[tokio::test]
async fn open_creates_parents_database_and_migrations() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested").join("crumbs.db");

    let database = Database::open(config(path.clone())).await?;

    assert!(path.is_file(), "database file was not created");
    let applied: i64 = query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(&database.pool)
        .await?;
    assert!(
        applied >= 1,
        "expected the embedded migrations to be applied"
    );

    let schema: String = query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'crumbs_sources'",
    )
    .fetch_one(&database.pool)
    .await?;
    assert!(
        schema.contains("STRICT"),
        "crumbs_sources was not created as a STRICT table: {schema}"
    );

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn reopen_preserves_data_and_does_not_reapply_migrations() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");

    let database = Database::open(config(path.clone())).await?;
    query("INSERT INTO crumbs_sources (id) VALUES (?)")
        .bind("usda")
        .execute(&database.pool)
        .await?;
    let applied_before: i64 =
        query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&database.pool)
            .await?;
    database.close().await;

    let reopened = Database::open(config(path)).await?;
    let sources: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_sources")
        .fetch_one(&reopened.pool)
        .await?;
    let applied_after: i64 =
        query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&reopened.pool)
            .await?;

    assert_eq!(sources, 1, "reopening lost existing data");
    assert_eq!(
        applied_after, applied_before,
        "reopening reapplied a completed migration"
    );

    reopened.close().await;
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn applies_pragmas_and_pool_limits_to_every_connection() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(config(directory.path().join("crumbs.db"))).await?;

    let options = database.pool.options();
    assert_eq!(options.get_max_connections(), 2);
    assert_eq!(options.get_min_connections(), 0);
    assert_eq!(options.get_idle_timeout(), Some(Duration::from_secs(60)));

    let mut first = database.pool.acquire().await?;
    let mut second = database.pool.acquire().await?;
    assert_eq!(database.pool.size(), 2);

    assert_connection_pragmas(&mut first).await?;
    assert_connection_pragmas(&mut second).await?;

    drop(first);
    drop(second);
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn enforces_foreign_keys() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(config(directory.path().join("crumbs.db"))).await?;

    query("CREATE TABLE fk_parent (id TEXT NOT NULL PRIMARY KEY) STRICT")
        .execute(&database.pool)
        .await?;
    query(
        "CREATE TABLE fk_child (
            parent_id TEXT NOT NULL REFERENCES fk_parent(id)
        ) STRICT",
    )
    .execute(&database.pool)
    .await?;

    let result = query("INSERT INTO fk_child (parent_id) VALUES (?)")
        .bind("missing")
        .execute(&database.pool)
        .await;

    assert!(result.is_err(), "foreign key violation was accepted");
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn fts5_is_available_in_the_bundled_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(config(directory.path().join("crumbs.db"))).await?;

    query("CREATE VIRTUAL TABLE fts_probe USING fts5(content)")
        .execute(&database.pool)
        .await?;
    query("INSERT INTO fts_probe (content) VALUES (?)")
        .bind("Crunchy apple")
        .execute(&database.pool)
        .await?;

    let matches: i64 = query_scalar("SELECT COUNT(*) FROM fts_probe WHERE fts_probe MATCH ?")
        .bind("apple")
        .fetch_one(&database.pool)
        .await?;

    assert_eq!(matches, 1, "FTS5 did not index the inserted text");
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn migration_checksum_mismatch_is_reported_without_a_handle() -> Result<(), Box<dyn StdError>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let database = Database::open(config(path.clone())).await?;

    query("UPDATE _sqlx_migrations SET checksum = X'00'")
        .execute(&database.pool)
        .await?;
    database.close().await;

    let error = match Database::open(config(path.clone())).await {
        Ok(database) => {
            database.close().await;
            return Err(io::Error::other("modified migration was accepted").into());
        }
        Err(error) => error,
    };

    assert!(matches!(error, Error::Migration { .. }));
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "migration error did not name the database path: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn invalid_parent_path_reports_the_directory() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let blocker = directory.path().join("blocker");
    fs::write(&blocker, b"not a directory")?;
    let path = blocker.join("nested").join("crumbs.db");

    let error = match Database::open(config(path)).await {
        Ok(database) => {
            database.close().await;
            return Err(io::Error::other("database opened below a regular file").into());
        }
        Err(error) => error,
    };

    assert!(matches!(error, Error::CreateDirectory { .. }));
    assert!(
        error.to_string().contains("blocker"),
        "directory error did not name the failing path: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn database_path_that_is_a_directory_reports_the_path() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    fs::create_dir(&path)?;

    let error = match Database::open(config(path.clone())).await {
        Ok(database) => {
            database.close().await;
            return Err(io::Error::other("a directory was accepted as a database file").into());
        }
        Err(error) => error,
    };

    assert!(matches!(error, Error::Open { .. }));
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "open error did not name the database path: {error}"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unwritable_parent_reports_the_database_path() -> Result<(), Box<dyn StdError>> {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir()?;
    let parent = directory.path().join("read-only");
    fs::create_dir(&parent)?;
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500))?;

    let permissions_enforced = fs::File::create(parent.join("probe")).is_err();
    let result = Database::open(config(parent.join("crumbs.db"))).await;
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;

    if !permissions_enforced {
        if let Ok(database) = result {
            database.close().await;
        }
        return Ok(());
    }

    let error = match result {
        Ok(database) => {
            database.close().await;
            return Err(io::Error::other("database opened in an unwritable directory").into());
        }
        Err(error) => error,
    };

    assert!(matches!(error, Error::Open { .. }));
    assert!(
        error.to_string().contains(&parent.display().to_string()),
        "open error did not name the database path: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn debug_does_not_expose_connection_details() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let database = Database::open(config(path.clone())).await?;

    let debug = format!("{database:?}");

    assert_eq!(debug, "Database { .. }");
    assert!(!debug.contains(&path.display().to_string()));
    database.close().await;
    Ok(())
}

#[test]
fn database_is_send_sync() {
    fn require_traits<T: Send + Sync>() {}
    require_traits::<Database>();
}

fn config(path: PathBuf) -> DatabaseConfig {
    DatabaseConfig { path }
}

async fn assert_connection_pragmas(
    connection: &mut SqliteConnection,
) -> Result<(), Box<dyn StdError>> {
    let journal_mode: String = query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *connection)
        .await?;
    let synchronous: i64 = query_scalar("PRAGMA synchronous")
        .fetch_one(&mut *connection)
        .await?;
    let foreign_keys: i64 = query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await?;
    let busy_timeout: i64 = query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *connection)
        .await?;
    let cache_size: i64 = query_scalar("PRAGMA cache_size")
        .fetch_one(&mut *connection)
        .await?;
    let mmap_size: i64 = query_scalar("PRAGMA mmap_size")
        .fetch_one(&mut *connection)
        .await?;
    let wal_autocheckpoint: i64 = query_scalar("PRAGMA wal_autocheckpoint")
        .fetch_one(&mut *connection)
        .await?;

    assert_eq!(journal_mode, "wal");
    assert_eq!(synchronous, 2, "expected synchronous=FULL");
    assert_eq!(foreign_keys, 1);
    assert_eq!(busy_timeout, 5000);
    assert_eq!(cache_size, -2048);
    assert_eq!(mmap_size, 0);
    assert_eq!(wal_autocheckpoint, 1000);
    Ok(())
}
