use sqlx::FromRow;

use crate::foods::{
    Dataset, Food, FoodCategory, FoodDescription, FoodId, FoodNotes, FoodSummary, Source,
    SourceFoodId, StoredFood,
};
use crate::nutrition_model::FoodData;

pub(super) type RowId = i64;

#[derive(FromRow)]
pub(super) struct StoredFoodRow {
    id: RowId,
    source: SourceText,
    source_food_id: SourceFoodId,
    dataset: DatasetText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    notes: Option<FoodNotes>,
    parsed_data: ParsedFoodDataText,
}

impl StoredFoodRow {
    pub(super) fn into_result(self) -> Result<StoredFood, sqlx::Error> {
        let dataset = self
            .dataset
            .parse::<Dataset>()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        let parsed_data = serde_json::from_str::<FoodData>(&self.parsed_data)
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        Ok(StoredFood {
            id: FoodId(self.id),
            food: Food {
                source: Source::Owned(self.source),
                source_food_id: self.source_food_id,
                dataset,
                description: self.description,
                category: self.category,
                notes: self.notes,
                parsed_data,
            },
        })
    }
}

#[derive(FromRow)]
pub(super) struct TagAssignmentRow {
    pub(super) tag_id: RowId,
    pub(super) name_key: String,
}

#[derive(FromRow)]
pub(super) struct FoodRow {
    id: RowId,
    source: SourceText,
    source_food_id: SourceFoodId,
    dataset: DatasetText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    notes: Option<FoodNotes>,
}

impl FoodRow {
    pub(super) fn into_summary(self) -> Result<FoodSummary, sqlx::Error> {
        let row = self;
        let dataset = row
            .dataset
            .parse::<Dataset>()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        Ok(FoodSummary {
            id: FoodId(row.id),
            source: row.source.into(),
            source_food_id: row.source_food_id,
            dataset,
            description: row.description,
            category: row.category,
            notes: row.notes,
        })
    }
}

type SourceText = String;
type DatasetText = String;
type ParsedFoodDataText = String;
