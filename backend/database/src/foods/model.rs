use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::nutrition_model::FoodData;

pub type Source = Cow<'static, str>;
pub type SourceRef<'a> = &'a str;
pub type SourceFoodId = String;
pub type SourceFoodIdRef<'a> = &'a str;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dataset {
    Common,
    Branded,
    Custom,
}

impl Dataset {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Branded => "branded",
            Self::Custom => "custom",
        }
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Dataset {
    type Err = InvalidDataset;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "common" => Ok(Self::Common),
            "branded" => Ok(Self::Branded),
            "custom" => Ok(Self::Custom),
            _ => Err(InvalidDataset(value.to_owned())),
        }
    }
}

impl From<Dataset> for String {
    fn from(value: Dataset) -> Self {
        value.as_str().to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid dataset identifier: {0}")]
pub struct InvalidDataset(String);

impl InvalidDataset {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub type FoodDescription = String;
pub type FoodCategory = String;
pub type FoodNotes = String;

#[derive(Debug, Serialize)]
pub struct Food {
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub dataset: Dataset,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
    pub notes: Option<FoodNotes>,
    pub parsed_data: FoodData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct FoodId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodSummary {
    pub id: FoodId,
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub dataset: Dataset,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
    pub notes: Option<FoodNotes>,
}

#[derive(Debug, Serialize)]
pub struct StoredFood {
    pub id: FoodId,
    #[serde(flatten)]
    pub food: Food,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i64")]
pub struct FoodLimit(i64);

impl FoodLimit {
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Default for FoodLimit {
    fn default() -> Self {
        Self(20)
    }
}

impl From<i64> for FoodLimit {
    fn from(value: i64) -> Self {
        Self(value.clamp(20, 1_000))
    }
}

#[derive(Debug)]
pub struct FoodStoreError(Box<dyn StdError + Send + Sync + 'static>);

impl fmt::Display for FoodStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0.as_ref(), formatter)
    }
}

impl StdError for FoodStoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.0.as_ref())
    }
}

impl FoodStoreError {
    pub(super) fn new(source: impl StdError + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

#[cfg(test)]
#[path = "../../unit-tests/foods/model.rs"]
mod tests;
