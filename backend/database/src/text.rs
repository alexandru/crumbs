//! Shared validated text values.

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};

/// Nonblank text, preserved exactly as supplied.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct NonBlankString(String);

impl NonBlankString {
    /// Creates text without changing its content.
    ///
    /// # Errors
    ///
    /// Returns [`BlankStringError`] if the text is empty or contains only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, BlankStringError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(BlankStringError);
        }

        Ok(Self(value))
    }

    /// Returns the text as supplied.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the owned text as supplied.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for NonBlankString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Text is empty or contains only whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("text must not be blank")]
pub struct BlankStringError;

#[cfg(test)]
#[path = "../unit-tests/text.rs"]
mod tests;
