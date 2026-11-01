use std::error::Error as StdError;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use sqlx::{SqliteConnection, query, query_scalar};

use super::{Database, DatabaseConfig, Error};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;

    Ok(())
}

#[test]
fn open_and_drop_with_time_only_runtime_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested").join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()?;

    runtime.block_on(async {
        let database = Database::open(config(path.clone())).await?;
        let applied: i64 = query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&database.pool)
            .await?;
        assert!(applied >= 1, "embedded migrations were not applied");
        #[cfg(target_os = "linux")]
        assert!(
            has_open_sqlite_fds(&path)?,
            "database has no open file handles"
        );

        database.close().await;
        #[cfg(target_os = "linux")]
        assert_sqlite_fds_released(&path)?;
        drop(database);

        Ok::<(), Box<dyn StdError>>(())
    })?;

    drop(runtime);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn reopen_preserves_data_and_does_not_reapply_migrations() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");

    let database = Database::open(config(path.clone())).await?;
    query("INSERT INTO crumbs_sources (id) VALUES (?)")
        .bind("test-source")
        .execute(&database.pool)
        .await?;
    let applied_before: i64 =
        query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&database.pool)
            .await?;
    database.close().await;
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;
    drop(database);
    let reopened = Database::open(config(path.clone())).await?;
    let sources: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_sources WHERE id = 'test-source'")
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
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;

    Ok(())
}

#[test]
fn open_and_close_in_current_thread_runtime() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested").join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let database = Database::open(config(path.clone())).await?;
        assert!(path.is_file(), "database file was not created");
        database.close().await;
        Ok::<(), Box<dyn StdError>>(())
    })?;
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn open_and_drop_in_local_set_run_until_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let local = tokio::task::LocalSet::new();

    runtime.block_on(local.run_until(async {
        let database = Database::open(config(path.clone())).await?;
        database.close().await;
        drop(database);
        Ok::<(), Box<dyn StdError>>(())
    }))?;
    assert_sqlite_fds_released(&path)?;
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn open_and_drop_in_local_set_task_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let local = tokio::task::LocalSet::new();
    let task_path = path.clone();
    let task = local.spawn_local(async move {
        let database = Database::open(config(task_path))
            .await
            .map_err(io::Error::other)?;
        database.close().await;
        drop(database);
        Ok::<(), io::Error>(())
    });

    runtime.block_on(local.run_until(async { task.await.map_err(io::Error::other)? }))?;
    assert_sqlite_fds_released(&path)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg(target_os = "linux")]
async fn close_waits_for_checked_out_connections_and_is_idempotent() -> Result<(), Box<dyn StdError>>
{
    use std::task::{Context, Poll, Waker};

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let database = Database::open(config(path.clone())).await?;
    let connection = database.pool.acquire().await?;

    {
        let mut close = std::pin::pin!(database.close());
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            std::future::Future::poll(close.as_mut(), &mut context),
            Poll::Pending
        ));

        drop(connection);
        close.await;
    }
    database.close().await;
    let ((), ()) = tokio::join!(database.close(), database.close());
    assert_sqlite_fds_released(&path)?;
    drop(database);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg(target_os = "linux")]
async fn dropping_database_is_nonblocking_and_pool_eventually_closes()
-> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let database = Database::open(config(path.clone())).await?;
    let connection = database.pool.acquire().await?;
    let close_event = database.pool.close_event();

    drop(database);
    drop(connection);
    close_event.await;
    wait_for_sqlite_fds_release(&path).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn cancelling_open_allows_reopen() -> Result<(), Box<dyn StdError>> {
    use std::task::{Context, Poll, Waker};

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    {
        let mut open = std::pin::pin!(Database::open(config(path.clone())));
        let mut context = Context::from_waker(Waker::noop());
        match std::future::Future::poll(open.as_mut(), &mut context) {
            Poll::Pending => {}
            Poll::Ready(Ok(database)) => drop(database),
            Poll::Ready(Err(error)) => return Err(error.into()),
        }
    }

    let database = Database::open(config(path.clone())).await?;
    database.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg(target_os = "linux")]
async fn dropping_database_releases_sqlite_and_preserves_committed_data()
-> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    {
        let database = Database::open(config(path.clone())).await?;
        query("INSERT INTO crumbs_sources (id) VALUES (?)")
            .bind("test-source")
            .execute(&database.pool)
            .await?;
        assert!(
            has_open_sqlite_fds(&path)?,
            "database has no open file handles"
        );
        database.close().await;
    }
    assert_sqlite_fds_released(&path)?;

    let reopened = Database::open(config(path.clone())).await?;
    let sources: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_sources WHERE id = 'test-source'")
        .fetch_one(&reopened.pool)
        .await?;
    assert_eq!(sources, 1, "dropping the database lost committed data");
    reopened.close().await;
    drop(reopened);
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg(target_os = "linux")]
async fn early_error_return_drops_database_and_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let (close_event_sender, close_event_receiver) = tokio::sync::oneshot::channel();
    let result = operation_that_returns_error(path.clone(), close_event_sender).await;

    assert_eq!(
        result.err().map(|error| error.to_string()).as_deref(),
        Some("operation failed after opening database")
    );
    close_event_receiver.await?.await;
    wait_for_sqlite_fds_release(&path).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg(target_os = "linux")]
async fn cancelling_operation_drops_database_and_releases_sqlite() -> Result<(), Box<dyn StdError>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let (opened_sender, opened_receiver) = tokio::sync::oneshot::channel();
    let (close_event_sender, close_event_receiver) = tokio::sync::oneshot::channel();
    let task_path = path.clone();
    let file_check_path = path.clone();
    let task = tokio::spawn(async move {
        let database = Database::open(config(task_path))
            .await
            .map_err(io::Error::other)?;
        let close_event = database.pool.close_event();
        let _ = close_event_sender.send(close_event);
        let mut transaction = database.pool.begin().await.map_err(io::Error::other)?;
        query("INSERT INTO crumbs_sources (id) VALUES (?)")
            .bind("cancelled-transaction")
            .execute(&mut *transaction)
            .await
            .map_err(io::Error::other)?;
        assert!(has_open_sqlite_fds(&file_check_path)?);
        let _ = opened_sender.send(());
        std::future::pending::<()>().await;
        Ok::<(), io::Error>(())
    });

    opened_receiver.await?;
    let close_event = close_event_receiver.await?;
    task.abort();
    match task.await {
        Err(error) if error.is_cancelled() => {}
        _ => return Err(io::Error::other("database task was not cancelled").into()),
    }
    close_event.await;
    wait_for_sqlite_fds_release(&path).await?;

    let reopened = Database::open(config(path.clone())).await?;
    let sources: i64 =
        query_scalar("SELECT COUNT(*) FROM crumbs_sources WHERE id = 'cancelled-transaction'")
            .fetch_one(&reopened.pool)
            .await?;
    assert_eq!(sources, 0, "cancelled transaction was committed");
    reopened.close().await;
    drop(reopened);
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn dropping_database_before_runtime_teardown_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let database = Database::open(config(path.clone())).await?;
        query("INSERT INTO crumbs_sources (id) VALUES (?)")
            .bind("test-source")
            .execute(&database.pool)
            .await?;
        assert!(
            has_open_sqlite_fds(&path)?,
            "database has no open file handles"
        );
        let close_event = database.pool.close_event();
        drop(database);
        close_event.await;
        wait_for_sqlite_fds_release(&path).await?;
        Ok::<(), Box<dyn StdError>>(())
    })?;
    drop(runtime);
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn dropping_database_outside_tokio_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let (database, close_event) = runtime.block_on(async {
        let database = Database::open(config(path.clone())).await?;
        query("INSERT INTO crumbs_sources (id) VALUES (?)")
            .bind("test-source")
            .execute(&database.pool)
            .await?;
        assert!(
            has_open_sqlite_fds(&path)?,
            "database has no open file handles"
        );
        let close_event = database.pool.close_event();
        Ok::<(Database, sqlx::pool::CloseEvent), Box<dyn StdError>>((database, close_event))
    })?;

    drop(database);
    runtime.block_on(async {
        close_event.await;
        wait_for_sqlite_fds_release(&path).await
    })?;
    drop(runtime);
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn dropping_database_inside_futures_executor_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let (database, close_event) = runtime.block_on(async {
        let database = Database::open(config(path.clone())).await?;
        query("INSERT INTO crumbs_sources (id) VALUES (?)")
            .bind("test-source")
            .execute(&database.pool)
            .await?;
        assert!(
            has_open_sqlite_fds(&path)?,
            "database has no open file handles"
        );
        let close_event = database.pool.close_event();
        Ok::<(Database, sqlx::pool::CloseEvent), Box<dyn StdError>>((database, close_event))
    })?;

    futures_executor::block_on(async {
        drop(database);
    });
    runtime.block_on(async {
        close_event.await;
        wait_for_sqlite_fds_release(&path).await
    })?;
    drop(runtime);
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn dropping_database_in_current_thread_runtime_releases_sqlite() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let (database, close_event) = runtime.block_on(async {
        let database = Database::open(config(path.clone())).await?;
        query("INSERT INTO crumbs_sources (id) VALUES (?)")
            .bind("test-source")
            .execute(&database.pool)
            .await?;
        assert!(
            has_open_sqlite_fds(&path)?,
            "database has no open file handles"
        );
        let close_event = database.pool.close_event();
        Ok::<(Database, sqlx::pool::CloseEvent), Box<dyn StdError>>((database, close_event))
    })?;
    let foreign_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    foreign_runtime.block_on(async {
        drop(database);
        close_event.await;
        wait_for_sqlite_fds_release(&path).await
    })?;
    drop(foreign_runtime);
    drop(runtime);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn migration_checksum_mismatch_is_reported_without_a_handle() -> Result<(), Box<dyn StdError>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let database = Database::open(config(path.clone())).await?;

    query("UPDATE _sqlx_migrations SET checksum = X'00'")
        .execute(&database.pool)
        .await?;
    #[cfg(target_os = "linux")]
    assert!(
        has_open_sqlite_fds(&path)?,
        "database has no open file handles"
    );
    database.close().await;
    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;
    drop(database);
    let error = match Database::open(config(path.clone())).await {
        Ok(_) => return Err(io::Error::other("modified migration was accepted").into()),
        Err(error) => error,
    };

    #[cfg(target_os = "linux")]
    assert_sqlite_fds_released(&path)?;

    assert!(matches!(error, Error::Migration { .. }));
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "migration error did not name the database path: {error}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn invalid_parent_path_reports_the_directory() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let blocker = directory.path().join("blocker");
    fs::write(&blocker, b"not a directory")?;
    let path = blocker.join("nested").join("crumbs.db");

    let error = match Database::open(config(path)).await {
        Ok(_) => return Err(io::Error::other("database opened below a regular file").into()),
        Err(error) => error,
    };

    assert!(matches!(error, Error::CreateDirectory { .. }));
    assert!(
        error.to_string().contains("blocker"),
        "directory error did not name the failing path: {error}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn database_path_that_is_a_directory_reports_the_path() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    fs::create_dir(&path)?;

    let error = match Database::open(config(path.clone())).await {
        Ok(_) => {
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
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
        drop(result);
        return Ok(());
    }

    let error = match result {
        Ok(_) => return Err(io::Error::other("database opened in an unwritable directory").into()),
        Err(error) => error,
    };

    assert!(matches!(error, Error::Open { .. }));
    assert!(
        error.to_string().contains(&parent.display().to_string()),
        "open error did not name the database path: {error}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn debug_does_not_expose_connection_details() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let database = Database::open(config(path.clone())).await?;

    let debug = format!("{database:?}");

    assert_eq!(debug, "Database { .. }");
    assert!(!debug.contains(&path.display().to_string()));
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

#[cfg(target_os = "linux")]
async fn operation_that_returns_error(
    path: PathBuf,
    close_event_sender: tokio::sync::oneshot::Sender<sqlx::pool::CloseEvent>,
) -> Result<(), io::Error> {
    let database = Database::open(config(path.clone()))
        .await
        .map_err(io::Error::other)?;
    let close_event = database.pool.close_event();
    query("INSERT INTO crumbs_sources (id) VALUES (?)")
        .bind("test-source")
        .execute(&database.pool)
        .await
        .map_err(io::Error::other)?;
    assert!(has_open_sqlite_fds(&path)?);
    let _ = close_event_sender.send(close_event);
    Err(io::Error::other("operation failed after opening database"))
}

#[cfg(target_os = "linux")]
async fn wait_for_sqlite_fds_release(path: &std::path::Path) -> Result<(), Box<dyn StdError>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !has_open_sqlite_fds(path)? {
                return Ok::<(), io::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(io::Error::other)??;
    Ok(())
}

#[cfg(target_os = "linux")]
fn assert_sqlite_fds_released(path: &std::path::Path) -> Result<(), Box<dyn StdError>> {
    if has_open_sqlite_fds(path)? {
        return Err(io::Error::other("SQLite file handles remained after database drop").into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn has_open_sqlite_fds(path: &std::path::Path) -> io::Result<bool> {
    let expected = [
        path.to_path_buf(),
        sidecar_path(path, "-wal"),
        sidecar_path(path, "-shm"),
    ];
    for entry in fs::read_dir("/proc/self/fd")? {
        let entry = entry?;
        let target = match fs::read_link(entry.path()) {
            Ok(target) => target,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if expected.iter().any(|path| {
            target == *path
                || target
                    .to_str()
                    .is_some_and(|target| target == format!("{} (deleted)", path.display()))
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn sidecar_path(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut sidecar_path = path.as_os_str().to_owned();
    sidecar_path.push(suffix);
    sidecar_path.into()
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
