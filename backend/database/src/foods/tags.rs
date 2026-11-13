//! Reusable tag names, catalog, and assignment contract.

use std::error::Error as StdError;
use std::future::Future;
use std::hash::{Hash, Hasher};

use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use super::model::FoodId;

/// Local identity of a reusable tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagId(pub i64);

/// A reusable tag, retaining the spelling used when it was first created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: TagId,
    pub name: TagName,
}

/// A nonblank tag name with surrounding whitespace removed.
///
/// Equality uses Unicode canonical caseless matching and preserves accents.
#[derive(Debug, Clone)]
pub struct TagName {
    display: TagDisplayName,
    key: TagKey,
}

impl TagName {
    /// Constructs a tag name.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTagName`] if trimming leaves no characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidTagName> {
        let display = value.as_ref().trim();
        if display.is_empty() {
            return Err(InvalidTagName);
        }
        let decomposed = display.nfd().collect::<String>();
        let key = UniCase::new(decomposed).to_folded_case().nfd().collect();
        Ok(Self {
            display: display.to_owned(),
            key,
        })
    }

    /// Returns the display spelling.
    pub fn as_str(&self) -> &str {
        &self.display
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }
}

impl PartialEq for TagName {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for TagName {}

impl Hash for TagName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

/// Tag names cannot be blank.
#[derive(Debug, thiserror::Error)]
#[error("tag name must not be blank")]
pub struct InvalidTagName;

/// Reads the tag catalog and manages food assignments.
pub trait TagStore: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Atomically replaces a food's assignments, creating missing tags.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the food is missing or replacement fails.
    /// Failed replacement preserves assignments and does not commit catalog changes.
    fn set_food_tags(
        &self,
        food: FoodId,
        tags: &[TagName],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Lists the tag catalog, including tags without current assignments.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if storage access or decoding fails.
    fn list_tags(&self) -> impl Future<Output = Result<Vec<Tag>, Self::Error>> + Send;
}

type TagDisplayName = String;
type TagKey = String;

#[cfg(test)]
#[path = "../../unit-tests/foods/tags.rs"]
mod tests;
