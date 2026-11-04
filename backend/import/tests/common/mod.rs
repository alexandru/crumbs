use std::error::Error;
use std::path::{Path, PathBuf};

use crumbs_database::{Database, DatabaseConfig};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

pub struct DatabaseFixture {
    pub directory: TempDir,
    pub database: Database,
    pub pool: SqlitePool,
}

impl DatabaseFixture {
    pub async fn open() -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("crumbs.db");
        let database = Database::open(DatabaseConfig { path: path.clone() }).await?;
        let pool = observer_pool(&path).await?;

        Ok(Self {
            directory,
            database,
            pool,
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
        self.database.close().await;
    }

    #[allow(dead_code)]
    pub async fn reopen(&mut self) -> Result<(), Box<dyn Error>> {
        self.close().await;
        let path = self.database_path();
        self.database = Database::open(DatabaseConfig { path: path.clone() }).await?;
        self.pool = observer_pool(&path).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn database_path(&self) -> PathBuf {
        self.directory.path().join("crumbs.db")
    }
}

async fn observer_pool(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}
