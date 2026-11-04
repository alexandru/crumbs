//! Streaming USDA import orchestration.
//!
//! USDA foods retain the exact `(source, source_food_id)` identity, where the
//! source is `usda` and the identifier is `fdcId`. Records with different IDs
//! remain separate even when their descriptions or GTINs match. Rows absent
//! from an input remain untouched, so USDA archive subsets coexist.

mod usda;

use std::fmt;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio::task::JoinError;

use self::usda::{ParseError, RecordPosition, StreamItem};
use crumbs_database::{Food, FoodWriter, Source, SourceFoodId};

pub use self::usda::RecordPosition as ImportRecordPosition;

/// A field named in an import diagnostic.
pub type DiagnosticField = String;
/// A reason reported for a sanitized or rejected food.
pub type DiagnosticReason = String;

/// Number of foods in an import outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoodCount(usize);

impl FoodCount {
    /// Returns the food count.
    pub const fn get(self) -> usize {
        self.0
    }

    fn increment(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}

/// Summary of committed, sanitized, and rejected foods.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    committed: FoodCount,
    sanitized: FoodCount,
    rejected: FoodCount,
}

impl ImportReport {
    /// Returns the number of foods committed by this import.
    pub const fn committed(self) -> FoodCount {
        self.committed
    }

    /// Returns the number of committed foods whose source data was sanitized.
    pub const fn sanitized(self) -> FoodCount {
        self.sanitized
    }

    /// Returns the number of food records rejected during this import.
    pub const fn rejected(self) -> FoodCount {
        self.rejected
    }
}

/// Kind of food-level diagnostic produced by an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The food was committed after one or more source values were omitted.
    Sanitized,
    /// The food record was skipped because its data could not be represented.
    Rejected,
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sanitized => formatter.write_str("sanitized"),
            Self::Rejected => formatter.write_str("rejected"),
        }
    }
}

/// One food-level event observed during a USDA import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDiagnostic {
    /// Food provider.
    pub source: Source,
    /// Provider's food identifier, when it could be read.
    pub source_food_id: Option<SourceFoodId>,
    /// One-based position in the archive.
    pub position: ImportRecordPosition,
    /// Whether the food was sanitized or rejected.
    pub kind: DiagnosticKind,
    /// Field affected by the diagnostic, when known.
    pub field: Option<DiagnosticField>,
    /// Human-readable explanation.
    pub reason: DiagnosticReason,
}

impl fmt::Display for ImportDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} food source={}", self.kind, self.source)?;
        if let Some(source_food_id) = &self.source_food_id {
            write!(formatter, " source_id={source_food_id}")?;
        } else {
            formatter.write_str(" source_id=unknown")?;
        }
        write!(formatter, " record={}", self.position)?;
        if let Some(field) = &self.field {
            write!(formatter, " field={field}")?;
        }
        write!(formatter, ": {}", self.reason)
    }
}

/// Error produced while importing a USDA FoodData Central file.
#[derive(Debug, thiserror::Error)]
#[error("{failure} for {} (committed {} foods)", path.display(), report.committed.get())]
pub struct ImportError {
    path: PathBuf,
    report: ImportReport,
    #[source]
    failure: Box<ImportFailure>,
}

/// Imports a USDA file into a food-store implementation, one food per commit.
///
/// Invalid food records are skipped and included in the returned report. Use
/// [`import_usda_with_diagnostics`] to observe food-level details.
///
/// # Errors
///
/// Returns an error for input, framing, worker, storage, or diagnostic failures.
pub async fn import_usda<S: FoodWriter + ?Sized>(
    store: &S,
    path: impl AsRef<Path>,
) -> Result<ImportReport, ImportError> {
    import_usda_with_diagnostics(store, path, |_| Ok(())).await
}

/// Imports a USDA file and reports sanitized or rejected foods as they occur.
///
/// The callback runs on the async consumer task. Its error stops processing;
/// counts returned by [`ImportError::report`] include all completed work.
///
/// # Errors
///
/// Returns an error for input, framing, worker, storage, or diagnostic failures.
pub async fn import_usda_with_diagnostics<S, F>(
    store: &S,
    path: impl AsRef<Path>,
    mut on_diagnostic: F,
) -> Result<ImportReport, ImportError>
where
    S: FoodWriter + ?Sized,
    F: FnMut(&ImportDiagnostic) -> io::Result<()>,
{
    let path = path.as_ref();
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|source| ImportError::open(path, source))?;
    let file = file.into_std().await;
    let (sender, receiver) = mpsc::channel(PRODUCER_QUEUE_CAPACITY);
    let producer = tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(file);
        usda::stream_foods(reader, &sender)
    });

    let consumer_result = consume(store, receiver, path, &mut on_diagnostic).await;
    let producer_result = producer.await;

    match consumer_result {
        Err(error) => Err(error),
        Ok(report) => match producer_result {
            Ok(Ok(())) => Ok(report),
            Ok(Err(source)) => Err(ImportError::stream(path, report, source)),
            Err(source) => Err(ImportError::worker(path, report, source)),
        },
    }
}

impl ImportError {
    /// Returns import counts completed before the failure.
    pub const fn report(&self) -> ImportReport {
        self.report
    }

    fn open(path: &Path, source: io::Error) -> Self {
        Self {
            path: path.to_path_buf(),
            report: ImportReport::default(),
            failure: Box::new(ImportFailure::Open { source }),
        }
    }

    fn stream(path: &Path, report: ImportReport, source: ParseError) -> Self {
        Self {
            path: path.to_path_buf(),
            report,
            failure: Box::new(ImportFailure::Stream { source }),
        }
    }

    fn worker(path: &Path, report: ImportReport, source: JoinError) -> Self {
        Self {
            path: path.to_path_buf(),
            report,
            failure: Box::new(ImportFailure::Worker { source }),
        }
    }

    fn write<E: std::error::Error + Send + Sync + 'static>(
        path: &Path,
        report: ImportReport,
        food: &Food,
        position: RecordPosition,
        source: E,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            report,
            failure: Box::new(ImportFailure::Write {
                fdc_id: food.source_food_id.clone(),
                position,
                source: Box::new(source),
            }),
        }
    }

    fn diagnostics(path: &Path, report: ImportReport, source: io::Error) -> Self {
        Self {
            path: path.to_path_buf(),
            report,
            failure: Box::new(ImportFailure::Diagnostics { source }),
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
        fdc_id: SourceFoodId,
        position: RecordPosition,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("failed to write USDA import diagnostics: {source}")]
    Diagnostics {
        #[source]
        source: io::Error,
    },
}

async fn consume<S, F>(
    store: &S,
    mut receiver: mpsc::Receiver<StreamItem>,
    path: &Path,
    on_diagnostic: &mut F,
) -> Result<ImportReport, ImportError>
where
    S: FoodWriter + ?Sized,
    F: FnMut(&ImportDiagnostic) -> io::Result<()>,
{
    let mut report = ImportReport::default();
    while let Some(item) = receiver.recv().await {
        match item {
            Ok(prepared) => {
                let mut prepared = prepared;
                let position = prepared.position;
                let sanitizations = std::mem::take(&mut prepared.sanitizations);
                let food = prepared.into_food();
                store
                    .upsert_food(&food)
                    .await
                    .map_err(|source| ImportError::write(path, report, &food, position, source))?;
                report.committed.increment();
                if !sanitizations.is_empty() {
                    report.sanitized.increment();
                }
                for sanitization in sanitizations {
                    let diagnostic = ImportDiagnostic {
                        source: food.source.clone(),
                        source_food_id: Some(food.source_food_id.clone()),
                        position,
                        kind: DiagnosticKind::Sanitized,
                        field: Some(sanitization.field),
                        reason: sanitization.reason,
                    };
                    on_diagnostic(&diagnostic)
                        .map_err(|source| ImportError::diagnostics(path, report, source))?;
                }
            }
            Err(rejected) => {
                let usda::RejectedFood {
                    position,
                    source_food_id,
                    field,
                    reason,
                } = rejected;
                report.rejected.increment();
                let diagnostic = ImportDiagnostic {
                    source: std::borrow::Cow::Borrowed("usda"),
                    source_food_id,
                    position,
                    kind: DiagnosticKind::Rejected,
                    field,
                    reason,
                };
                on_diagnostic(&diagnostic)
                    .map_err(|source| ImportError::diagnostics(path, report, source))?;
            }
        }
    }
    Ok(report)
}

const PRODUCER_QUEUE_CAPACITY: usize = 1;
