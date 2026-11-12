use std::error::Error as StdError;

/// Errors returned by profile validation and storage operations.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// A profile value or request is invalid.
    #[error("{0}")]
    InvalidInput(String),
    /// The account already has a profile with an equivalent name.
    #[error("profile name already exists for this account")]
    DuplicateName,
    /// Profile-derived arithmetic cannot be represented.
    #[error("profile calculation failed: {0}")]
    Calculation(String),
    /// Stored profile data cannot be decoded or violates its invariants.
    #[error("stored profile data is invalid")]
    InvalidStoredData,
    /// Profile storage failed.
    #[error("profile storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    /// The application clock could not provide a usable timestamp.
    #[error("profile clock operation failed")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

impl ProfileError {
    /// Returns the stable machine-readable category for this error.
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
