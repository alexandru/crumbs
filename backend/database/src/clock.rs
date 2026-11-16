use std::time::SystemTime;

pub(crate) type UnixMillis = i64;

/// Clock is a way to inject time in a way that can be overriden and
/// that is testable.
///
/// Getting the current time is always a side-effectful operation with
/// non-deterministic results. It helps if the time generator is given
/// as a parameter, for making tests more deterministic, but not only.
pub(crate) struct Clock {
    source: ClockSource,
}

impl Default for Clock {
    fn default() -> Self {
        Self::with_source(SystemTime::now)
    }
}

impl Clock {
    pub(crate) fn with_source(source: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }

    pub(crate) fn now(&self) -> Result<UnixMillis, ClockError> {
        unix_millis((self.source)())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClockError {
    #[error("system time precedes the Unix epoch")]
    BeforeEpoch,
    #[error("system time exceeds the signed 64-bit millisecond range")]
    OutOfRange,
}

type ClockSource = Box<dyn Fn() -> SystemTime + Send + Sync>;

fn unix_millis(time: SystemTime) -> Result<UnixMillis, ClockError> {
    let elapsed = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| ClockError::BeforeEpoch)?;
    i64::try_from(elapsed.as_millis()).map_err(|_| ClockError::OutOfRange)
}
