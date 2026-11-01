//! Application clock for persistence timestamps.

use std::time::SystemTime;

/// Milliseconds since the Unix epoch, as stored in persistence timestamps.
pub(crate) type UnixMillis = i64;

/// Application clock used for persistence timestamps.
///
/// The default source is the system clock. Unit tests may inject a controlled source.
#[derive(Default)]
pub(crate) struct Clock {
    #[cfg(test)]
    source: ClockSource,
}

impl Clock {
    pub(crate) fn now(&self) -> Result<UnixMillis, ClockError> {
        #[cfg(test)]
        let time = match &self.source {
            ClockSource::System => SystemTime::now(),
            ClockSource::Controlled(controlled) => controlled(),
        };
        #[cfg(not(test))]
        let time = SystemTime::now();

        unix_millis(time)
    }
}

#[cfg(test)]
impl Clock {
    pub(crate) fn controlled(source: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        Self {
            source: ClockSource::Controlled(Box::new(source)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClockError {
    #[error("system time precedes the Unix epoch")]
    BeforeEpoch,
    #[error("system time exceeds the signed 64-bit millisecond range")]
    OutOfRange,
}

#[cfg(test)]
#[derive(Default)]
enum ClockSource {
    #[default]
    System,
    Controlled(Box<dyn Fn() -> SystemTime + Send + Sync>),
}

fn unix_millis(time: SystemTime) -> Result<UnixMillis, ClockError> {
    let elapsed = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| ClockError::BeforeEpoch)?;
    i64::try_from(elapsed.as_millis()).map_err(|_| ClockError::OutOfRange)
}
