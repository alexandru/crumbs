use std::error::Error as StdError;

use crate::profiles::ProfileError;

#[derive(Debug, thiserror::Error)]
pub enum DailyStatusError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("stored daily-status data is invalid")]
    InvalidStoredData,
    #[error("daily-status storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error("daily-status clock operation failed")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error(transparent)]
    Profile(#[from] ProfileError),
}

impl DailyStatusError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::InvalidStoredData => "invalid_stored_data",
            Self::Storage(_) => "storage",
            Self::Clock(_) => "clock",
            Self::Profile(error) => error.code(),
        }
    }

    pub(crate) fn storage(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(source))
    }

    pub(crate) fn clock(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Clock(Box::new(source))
    }
}
