//! Source-independent food storage and tag membership.

use std::borrow::Cow;
use std::error::Error as StdError;
use std::future::Future;
use std::hash::{Hash, Hasher};

use caseless::Caseless;
use serde_json::value::RawValue;
use unicode_normalization::UnicodeNormalization;

mod sqlite;

/// Persists source-scoped foods independently of their import format.
pub trait FoodWriter: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Atomically inserts or updates a food, preserving its local ID and tags.
    fn upsert_food(&self, food: &Food) -> impl Future<Output = Result<FoodId, Self::Error>> + Send;
}

/// Looks up foods and manages tag assignments for a food writer.
pub trait FoodStore: FoodWriter {
    /// Looks up food metadata by its exact source-scoped identity.
    fn food_by_source(
        &self,
        source: &Source,
        source_food_id: &SourceFoodId,
    ) -> impl Future<Output = Result<Option<FoodSummary>, Self::Error>> + Send;

    /// Atomically replaces a food's assignments, creating missing tags.
    fn set_food_tags(
        &self,
        food: FoodId,
        tags: &[TagName],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Lists foods with this tag, using case-insensitive exact membership.
    fn foods_with_tag(
        &self,
        tag: &TagName,
    ) -> impl Future<Output = Result<Vec<FoodSummary>, Self::Error>> + Send;

    /// Lists the tag catalog, including tags with no current assignments.
    fn list_tags(&self) -> impl Future<Output = Result<Vec<Tag>, Self::Error>> + Send;
}

/// Provider identifier; providers may use static or dynamically supplied names.
pub type Source = Cow<'static, str>;
/// Opaque identifier assigned to a food by its source.
pub type SourceFoodId = String;
/// Source-defined kind of food.
pub type FoodType = Cow<'static, str>;
/// Display description supplied by a source.
pub type FoodDescription = String;
/// Optional display category supplied by a source.
pub type FoodCategory = String;

/// A complete food entry supplied by any source.
#[derive(Debug)]
pub struct Food {
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub food_type: FoodType,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
    /// Original source JSON, including its original numeric spelling.
    pub source_data: Box<RawValue>,
}

/// Local food identity, independent of source identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FoodId(pub i64);

/// Food metadata returned by tag lookup, without loading the source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodSummary {
    pub id: FoodId,
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub food_type: FoodType,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
}

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
        let key = display.chars().nfd().default_case_fold().nfd().collect();
        Ok(Self {
            display: display.to_owned(),
            key,
        })
    }

    /// Returns the display spelling.
    pub fn as_str(&self) -> &str {
        &self.display
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

type TagDisplayName = String;
type TagKey = String;

#[cfg(test)]
#[path = "../unit-tests/foods.rs"]
mod tests;
