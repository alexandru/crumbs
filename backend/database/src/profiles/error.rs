use std::error::Error as StdError;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("profile name already exists for this account")]
    DuplicateName,
    #[error("profile calculation failed: {0}")]
    Calculation(String),
    #[error("stored profile data is invalid")]
    InvalidStoredData,
    #[error("profile storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error("profile clock operation failed")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

impl ProfileError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::DuplicateName => "duplicate_name",
            Self::Calculation(_) => "calculation",
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
