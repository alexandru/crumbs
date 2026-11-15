use std::error::Error as StdError;
use std::future::Future;
use std::hash::{Hash, Hasher};

use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use super::model::FoodId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: TagId,
    pub name: TagName,
}

#[derive(Debug, Clone)]
pub struct TagName {
    display: TagDisplayName,
    key: TagKey,
}

impl TagName {
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

#[derive(Debug, thiserror::Error)]
#[error("tag name must not be blank")]
pub struct InvalidTagName;

pub trait TagStore: Sync {
    type Error: StdError + Send + Sync + 'static;

    fn set_food_tags(
        &self,
        food: FoodId,
        tags: &[TagName],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn list_tags(&self) -> impl Future<Output = Result<Vec<Tag>, Self::Error>> + Send;
}

type TagDisplayName = String;
type TagKey = String;

#[cfg(test)]
#[path = "../../unit-tests/foods/tags.rs"]
mod tests;
