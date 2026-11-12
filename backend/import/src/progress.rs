use std::future::Future;
use std::path::Path;
use std::time::Duration;

use tokio::time::{Instant, Interval, MissedTickBehavior};

use crate::{ImportError, ImportReport};

pub(super) enum Phase {
    WaitingForRecord,
    Writing,
}

impl Phase {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::WaitingForRecord => "waiting_for_record",
            Self::Writing => "writing",
        }
    }
}

pub(super) struct Progress {
    started: Instant,
    interval: Interval,
}

impl Progress {
    pub(super) fn new(path: &Path) -> Self {
        let started = Instant::now();
        let mut interval =
            tokio::time::interval_at(started + Duration::from_secs(5), Duration::from_secs(5));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        tracing::info!(event = "import_started", path = %path.display());
        Self { started, interval }
    }

    pub(super) async fn observe<F: Future>(
        &mut self,
        work: F,
        report: ImportReport,
        phase: Phase,
    ) -> F::Output {
        tokio::pin!(work);
        loop {
            tokio::select! {
                result = &mut work => return result,
                _ = self.interval.tick() => {
                    let elapsed_secs = self.started.elapsed().as_secs_f64();
                    let committed = report.committed().get();
                    tracing::info!(
                        event = "import_progress",
                        phase = phase.as_str(),
                        committed,
                        sanitized = report.sanitized().get(),
                        rejected = report.rejected().get(),
                        elapsed_secs,
                        foods_per_sec = committed as f64 / elapsed_secs
                    );
                }
            }
        }
    }

    pub(super) fn finish(&self, result: &Result<ImportReport, ImportError>) {
        let elapsed_secs = self.started.elapsed().as_secs_f64();
        match result {
            Ok(report) => tracing::info!(
                event = "import_completed",
                committed = report.committed().get(),
                sanitized = report.sanitized().get(),
                rejected = report.rejected().get(),
                elapsed_secs
            ),
            Err(error) => {
                let report = error.report();
                tracing::error!(
                    event = "import_failed",
                    committed = report.committed().get(),
                    sanitized = report.sanitized().get(),
                    rejected = report.rejected().get(),
                    elapsed_secs
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "../unit-tests/progress.rs"]
mod tests;
