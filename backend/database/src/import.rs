//! Streaming USDA import orchestration.
//!
//! USDA FoodData Central records are identified by the exact `(source,
//! source_food_id)` pair, with source fixed to `usda` and `source_food_id` set
//! to the record's `fdcId`. A record with a different `fdcId` is a separate
//! food identity: the importer never merges records by description, GTIN, or
//! other metadata. Rows absent from an input are left untouched, so Foundation,
//! branded, and other USDA subsets coexist.
//!
//! Parsing runs on a blocking worker and hands one prepared record at a time to
//! the asynchronous consumer. Each successfully persisted food commits in its
//! own transaction.

use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tokio::task::JoinError;

use crate::foods;
use crate::usda_import::{FdcId, ParseError, PreparedFood, RecordPosition, stream_foods};

/// Number of foods committed by an import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportedFoods(usize);

impl ImportedFoods {
    /// Returns the committed food count.
    pub const fn get(self) -> usize {
        self.0
    }

    fn increment(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}

/// Result of a successful USDA import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    committed: ImportedFoods,
}

impl ImportReport {
    /// Returns the number of foods committed by this import.
    pub const fn committed(self) -> ImportedFoods {
        self.committed
    }
}

/// Error produced while importing a USDA FoodData Central file.
#[derive(Debug, thiserror::Error)]
#[error("{failure} for {} (committed {} foods)", path.display(), committed.get())]
pub struct ImportError {
    path: PathBuf,
    committed: ImportedFoods,
    #[source]
    failure: Box<ImportFailure>,
}

pub(crate) async fn import_usda(
    pool: &SqlitePool,
    path: &Path,
) -> Result<ImportReport, ImportError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|source| ImportError::open(path, source))?;
    let file = file.into_std().await;
    let (sender, mut receiver) = mpsc::channel(PRODUCER_QUEUE_CAPACITY);
    let producer = tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(file);
        stream_foods(reader, &sender)
    });

    let mut committed = ImportedFoods::default();
    let consumer_result = consume(pool, &mut receiver, path, &mut committed).await;
    drop(receiver);
    let producer_result = producer.await;

    consumer_result?;

    match producer_result {
        Ok(Ok(())) => Ok(ImportReport { committed }),
        Ok(Err(source)) => Err(ImportError::stream(path, committed, source)),
        Err(source) => Err(ImportError::worker(path, committed, source)),
    }
}

impl ImportError {
    fn open(path: &Path, source: io::Error) -> Self {
        Self {
            path: path.to_path_buf(),
            committed: ImportedFoods::default(),
            failure: Box::new(ImportFailure::Open { source }),
        }
    }

    fn stream(path: &Path, committed: ImportedFoods, source: ParseError) -> Self {
        Self {
            path: path.to_path_buf(),
            committed,
            failure: Box::new(ImportFailure::Stream { source }),
        }
    }

    fn worker(path: &Path, committed: ImportedFoods, source: JoinError) -> Self {
        Self {
            path: path.to_path_buf(),
            committed,
            failure: Box::new(ImportFailure::Worker { source }),
        }
    }

    fn write(
        path: &Path,
        committed: ImportedFoods,
        food: &PreparedFood,
        source: sqlx::Error,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            committed,
            failure: Box::new(ImportFailure::Write {
                fdc_id: food.fdc_id.clone(),
                position: food.position,
                source,
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ImportFailure {
    #[error("failed to open the USDA input file: {source}")]
    Open {
        #[source]
        source: io::Error,
    },
    #[error("failed to stream the USDA input: {source}")]
    Stream {
        #[source]
        source: ParseError,
    },
    #[error("the USDA parser worker failed: {source}")]
    Worker {
        #[source]
        source: JoinError,
    },
    #[error("failed to persist USDA food {fdc_id} at record {position}: {source}")]
    Write {
        fdc_id: FdcId,
        position: RecordPosition,
        #[source]
        source: sqlx::Error,
    },
}

async fn consume(
    pool: &SqlitePool,
    receiver: &mut mpsc::Receiver<PreparedFood>,
    path: &Path,
    committed: &mut ImportedFoods,
) -> Result<(), ImportError> {
    while let Some(food) = receiver.recv().await {
        persist(pool, &food)
            .await
            .map_err(|source| ImportError::write(path, *committed, &food, source))?;
        committed.increment();
    }

    Ok(())
}

async fn persist(pool: &SqlitePool, food: &PreparedFood) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    foods::upsert_usda_food(&mut transaction, food).await?;
    transaction.commit().await
}

const PRODUCER_QUEUE_CAPACITY: usize = 1;

#[cfg(test)]
#[path = "../unit-tests/import.rs"]
mod tests;
