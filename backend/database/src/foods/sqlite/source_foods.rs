use super::rows::FoodRow;
use crate::clock::UnixMillis;
use crate::foods::{
    Food, FoodId, FoodLookup, FoodStoreError, FoodSummary, FoodWriter, SourceFoodIdRef, SourceRef,
};
use crate::runtime::Database;

impl FoodWriter for Database {
    type Error = FoodStoreError;

    async fn upsert_food(&self, food: &Food) -> Result<FoodId, Self::Error> {
        let operation: Result<FoodId, sqlx::Error> = async {
        let now: UnixMillis = self.clock.now()?;
        let parsed_data = serde_json::to_string(&food.parsed_data)
            .map_err(|source| sqlx::Error::Encode(Box::new(source)))?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO crumbs_sources (id, created_at) VALUES (?, ?)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(food.source.as_ref())
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let id = sqlx::query_scalar(
            "INSERT INTO crumbs_foods (
                source, source_food_id, dataset, description, category, notes, parsed_data, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (source, source_food_id) DO UPDATE SET
                dataset = excluded.dataset,
                description = excluded.description,
                category = excluded.category,
                notes = excluded.notes,
                parsed_data = excluded.parsed_data,
                updated_at = ?
            RETURNING id",
        )
        .bind(food.source.as_ref())
        .bind(&food.source_food_id)
        .bind(food.dataset.as_str())
        .bind(&food.description)
        .bind(food.category.as_deref())
        .bind(food.notes.as_deref())
        .bind(parsed_data)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(FoodId(id))

        }.await;
        operation.map_err(FoodStoreError::new)
    }
}

impl FoodLookup for Database {
    type Error = FoodStoreError;

    async fn food_by_source(
        &self,
        source: SourceRef<'_>,
        source_food_id: SourceFoodIdRef<'_>,
    ) -> Result<Option<FoodSummary>, Self::Error> {
        let operation: Result<Option<FoodSummary>, sqlx::Error> = async {
            let row = sqlx::query_as::<_, FoodRow>(
                "SELECT id, source, source_food_id, dataset, description, category, notes
             FROM crumbs_foods WHERE source = ? AND source_food_id = ?",
            )
            .bind(source)
            .bind(source_food_id)
            .fetch_optional(&self.pool)
            .await?;
            row.map(FoodRow::into_summary).transpose()
        }
        .await;
        operation.map_err(FoodStoreError::new)
    }
}

#[cfg(test)]
#[path = "../../../unit-tests/foods/sqlite/source_foods.rs"]
mod tests;
