use std::error::Error as StdError;

#[derive(Debug, thiserror::Error)]
pub enum ProfileLogError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("stored profile-log data is invalid")]
    InvalidStoredData,
    #[error("profile-log storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error("profile-log clock operation failed")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

impl ProfileLogError {
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
