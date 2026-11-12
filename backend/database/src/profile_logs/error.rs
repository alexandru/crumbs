use std::error::Error as StdError;

/// Errors returned by profile-log validation and storage operations.
#[derive(Debug, thiserror::Error)]
pub enum ProfileLogError {
    /// A log value or query is invalid.
    #[error("{0}")]
    InvalidInput(String),
    /// Stored profile-log data violates its public model.
    #[error("stored profile-log data is invalid")]
    InvalidStoredData,
    /// Profile-log storage failed.
    #[error("profile-log storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    /// The application clock could not provide a usable time.
    #[error("profile-log clock operation failed")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

impl ProfileLogError {
    /// Returns the stable machine-readable category for this error.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::InvalidStoredData => "invalid_stored_data",
            Self::Storage(_) => "storage",
            Self::Clock(_) => "clock",
        }
    }

    pub(crate) fn storage(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(source))
    }

    pub(crate) fn clock(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Clock(Box::new(source))
    }
}
