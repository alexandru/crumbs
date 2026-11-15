mod progress;
mod usda;

use std::fmt;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio::task::JoinError;

use self::usda::{ParseError, RecordPosition, StreamItem};
use crumbs_database::foods::{FoodWriter, Source, SourceFoodId};

pub use self::usda::RecordPosition as ImportRecordPosition;

pub type DiagnosticField = String;
pub type DiagnosticReason = String;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoodCount(usize);

impl FoodCount {
    pub const fn get(self) -> usize {
        self.0
    }

    fn increment(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    committed: FoodCount,
    sanitized: FoodCount,
    rejected: FoodCount,
}

impl ImportReport {
    pub const fn committed(self) -> FoodCount {
        self.committed
    }

    pub const fn sanitized(self) -> FoodCount {
        self.sanitized
    }

    pub const fn rejected(self) -> FoodCount {
        self.rejected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Sanitized,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDiagnostic {
    pub source: Source,
    pub source_food_id: Option<SourceFoodId>,
    pub position: ImportRecordPosition,
    pub kind: DiagnosticKind,
    pub field: Option<DiagnosticField>,
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

#[derive(Debug, thiserror::Error)]
#[error("{failure} for {} (committed {} foods)", path.display(), report.committed.get())]
pub struct ImportError {
    path: PathBuf,
    report: ImportReport,
    #[source]
    failure: Box<ImportFailure>,
}

pub async fn import_usda<S: FoodWriter + ?Sized>(
    store: &S,
    path: impl AsRef<Path>,
) -> Result<ImportReport, ImportError> {
    import_usda_with_diagnostics(store, path, |_| Ok(())).await
}

pub async fn import_usda_with_diagnostics<S, F>(
    store: &S,
    path: impl AsRef<Path>,
    on_diagnostic: F,
) -> Result<ImportReport, ImportError>
where
    S: FoodWriter + ?Sized,
    F: FnMut(&ImportDiagnostic) -> io::Result<()>,
{
    let path = path.as_ref();
    let mut progress = progress::Progress::new(path);
    let result = import_inner(store, path, on_diagnostic, &mut progress).await;
    progress.finish(&result);
    result
}

impl ImportError {
    pub const fn report(&self) -> ImportReport {
        self.report
    }

    fn new(path: &Path, report: ImportReport, failure: ImportFailure) -> Self {
        Self {
            path: path.to_path_buf(),
            report,
            failure: Box::new(failure),
        }
    }
}

async fn import_inner<S, F>(
    store: &S,
    path: &Path,
    mut on_diagnostic: F,
    progress: &mut progress::Progress,
) -> Result<ImportReport, ImportError>
where
    S: FoodWriter + ?Sized,
    F: FnMut(&ImportDiagnostic) -> io::Result<()>,
{
    let file = tokio::fs::File::open(path).await.map_err(|source| {
        ImportError::new(
            path,
            ImportReport::default(),
            ImportFailure::Open { source },
        )
    })?;
    let file = file.into_std().await;
    let (sender, receiver) = mpsc::channel(PRODUCER_QUEUE_CAPACITY);
    let producer = tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(file);
        usda::stream_foods(reader, &sender)
    });

    let consumer_result = consume(store, receiver, path, &mut on_diagnostic, progress).await;
    let producer_result = producer.await;
    let report = consumer_result?;

    match producer_result {
        Ok(Ok(())) => Ok(report),
        Ok(Err(source)) => Err(ImportError::new(
            path,
            report,
            ImportFailure::Stream { source },
        )),
        Err(source) => Err(ImportError::new(
            path,
            report,
            ImportFailure::Worker { source },
        )),
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
    progress: &mut progress::Progress,
) -> Result<ImportReport, ImportError>
where
    S: FoodWriter + ?Sized,
    F: FnMut(&ImportDiagnostic) -> io::Result<()>,
{
    let mut report = ImportReport::default();
    while let Some(item) = progress
        .observe(receiver.recv(), report, progress::Phase::WaitingForRecord)
        .await
    {
        match item {
            Ok(prepared) => {
                let mut prepared = prepared;
                let position = prepared.position;
                let sanitizations = std::mem::take(&mut prepared.sanitizations);
                let food = prepared.into_food();
                progress
                    .observe(store.upsert_food(&food), report, progress::Phase::Writing)
                    .await
                    .map_err(|source| {
                        ImportError::new(
                            path,
                            report,
                            ImportFailure::Write {
                                fdc_id: food.source_food_id.clone(),
                                position,
                                source: Box::new(source),
                            },
                        )
                    })?;
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
                    on_diagnostic(&diagnostic).map_err(|source| {
                        ImportError::new(path, report, ImportFailure::Diagnostics { source })
                    })?;
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
                on_diagnostic(&diagnostic).map_err(|source| {
                    ImportError::new(path, report, ImportFailure::Diagnostics { source })
                })?;
            }
        }
    }
    Ok(report)
}

const PRODUCER_QUEUE_CAPACITY: usize = 1;
