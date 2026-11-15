use std::error::Error as StdError;
use std::future::Future;

use super::model::{Food, FoodId, FoodSummary, SourceFoodIdRef, SourceRef};

pub trait FoodWriter: Sync {
    type Error: StdError + Send + Sync + 'static;

    fn upsert_food(&self, food: &Food) -> impl Future<Output = Result<FoodId, Self::Error>> + Send;
}

pub trait FoodLookup: Sync {
    type Error: StdError + Send + Sync + 'static;

    fn food_by_source(
        &self,
        source: SourceRef<'_>,
        source_food_id: SourceFoodIdRef<'_>,
    ) -> impl Future<Output = Result<Option<FoodSummary>, Self::Error>> + Send;
}
