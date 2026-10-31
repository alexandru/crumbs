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

mod usda;

use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use tokio::runtime::{Handle, TryCurrentError};
use tokio::sync::mpsc;
use tokio::task::JoinError;

use self::usda::{ParseError, PreparedFood, RecordPosition};
use crate::{Food, FoodWriter, SourceFoodId};

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

/// Imports a USDA file into a food-store implementation, one food per commit.
///
/// # Errors
///
/// Reports a missing Tokio runtime or input, parsing, worker, or storage
/// failures with committed progress.
pub async fn import_usda<S: FoodWriter + ?Sized>(
    store: &S,
    path: impl AsRef<Path>,
) -> Result<ImportReport, ImportError> {
    let path = path.as_ref();
    Handle::try_current().map_err(|source| ImportError::runtime(path, source))?;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|source| ImportError::open(path, source))?;
    let file = file.into_std().await;
    let (sender, receiver) = mpsc::channel(PRODUCER_QUEUE_CAPACITY);
    let producer = tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(file);
        usda::stream_foods(reader, &sender)
    });

    let consumer_result = consume(store, receiver, path).await;
    let producer_result = producer.await;

    let committed = consumer_result?;

    match producer_result {
        Ok(Ok(())) => Ok(ImportReport { committed }),
        Ok(Err(source)) => Err(ImportError::stream(path, committed, source)),
        Err(source) => Err(ImportError::worker(path, committed, source)),
    }
}

impl ImportError {
    fn runtime(path: &Path, source: TryCurrentError) -> Self {
        Self {
            path: path.to_path_buf(),
            committed: ImportedFoods::default(),
            failure: Box::new(ImportFailure::Runtime { source }),
        }
    }

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

    fn write<E: std::error::Error + Send + Sync + 'static>(
        path: &Path,
        committed: ImportedFoods,
        food: &Food,
        position: RecordPosition,
        source: E,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            committed,
            failure: Box::new(ImportFailure::Write {
                fdc_id: food.source_food_id.clone(),
                position,
                source: Box::new(source),
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ImportFailure {
    #[error("no Tokio runtime is active: {source}")]
    Runtime {
        #[source]
        source: TryCurrentError,
    },
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
        fdc_id: SourceFoodId,
        position: RecordPosition,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

async fn consume<S: FoodWriter + ?Sized>(
    store: &S,
    mut receiver: mpsc::Receiver<PreparedFood>,
    path: &Path,
) -> Result<ImportedFoods, ImportError> {
    let mut committed = ImportedFoods::default();
    while let Some(food) = receiver.recv().await {
        let position = food.position;
        let food = food.into_food();
        store
            .upsert_food(&food)
            .await
            .map_err(|source| ImportError::write(path, committed, &food, position, source))?;
        committed.increment();
    }

    Ok(committed)
}

const PRODUCER_QUEUE_CAPACITY: usize = 1;

#[cfg(test)]
#[path = "../../unit-tests/import/mod.rs"]
mod tests;
