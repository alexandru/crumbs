//! Reusable tag names, catalog, and assignment contract.

use std::error::Error as StdError;
use std::future::Future;

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

crate::text::name_newtype! {
    /// A nonblank tag name with surrounding whitespace removed.
    ///
    /// Equality uses Unicode canonical caseless matching and preserves accents.
    pub struct TagName;
    /// Constructs a tag name.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTagName`] if trimming leaves no characters.
    fn new -> InvalidTagName;
    blank = InvalidTagName;
    pub(super) fn key;
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

#[cfg(test)]
#[path = "../../unit-tests/foods/tags.rs"]
mod tests;
