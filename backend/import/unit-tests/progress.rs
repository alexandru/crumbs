use std::error::Error;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("capture lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn emits_progress_while_each_phase_is_pending_and_stops_after_return() -> Result<(), Box<dyn Error>>
{
    let capture = Capture(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(capture.clone())
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()?;
    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let mut progress = Progress::new(Path::new("fixture.json"));
            let report = ImportReport {
                committed: crate::FoodCount(3),
                sanitized: crate::FoodCount(2),
                rejected: crate::FoodCount(1),
            };
            for phase in [Phase::WaitingForRecord, Phase::Writing] {
                let pending = std::future::pending::<()>();
                assert!(
                    tokio::time::timeout(
                        Duration::from_secs(11),
                        progress.observe(pending, report, phase)
                    )
                    .await
                    .is_err()
                );
            }
            assert_eq!(
                progress.observe(async { 7 }, report, Phase::Writing).await,
                7
            );
            tokio::time::advance(Duration::from_secs(20)).await;
        })
    });
    let output = String::from_utf8(
        capture
            .0
            .lock()
            .map_err(|_| io::Error::other("capture lock poisoned"))?
            .clone(),
    )?;
    assert!(output.contains("import_started"));
    assert_eq!(output.matches("import_progress").count(), 4);
    assert!(output.contains("phase=\"waiting_for_record\""));
    assert!(output.contains("phase=\"writing\""));
    assert!(output.contains("committed=3"));
    assert!(output.contains("sanitized=2"));
    assert!(output.contains("rejected=1"));
    Ok(())
}

#[test]
fn reports_completed_and_failed_totals() -> Result<(), Box<dyn Error>> {
    let capture = Capture(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(capture.clone())
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()?;
    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let report = ImportReport {
                committed: crate::FoodCount(3),
                sanitized: crate::FoodCount(2),
                rejected: crate::FoodCount(1),
            };
            let progress = Progress::new(Path::new("fixture.json"));
            progress.finish(&Ok(report));
            let error = ImportError::new(
                Path::new("fixture.json"),
                report,
                crate::ImportFailure::Open {
                    source: io::Error::other("test failure"),
                },
            );
            progress.finish(&Err(error));
        })
    });
    let output = String::from_utf8(
        capture
            .0
            .lock()
            .map_err(|_| io::Error::other("capture lock poisoned"))?
            .clone(),
    )?;
    assert_eq!(output.matches("import_started").count(), 1);
    assert_eq!(output.matches("import_completed").count(), 1);
    assert_eq!(output.matches("import_failed").count(), 1);
    assert_eq!(output.matches("committed=3").count(), 2);
    assert_eq!(output.matches("sanitized=2").count(), 2);
    assert_eq!(output.matches("rejected=1").count(), 2);
    Ok(())
}
