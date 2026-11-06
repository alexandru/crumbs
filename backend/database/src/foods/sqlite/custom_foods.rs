use super::rows::{RowId, StoredFoodRow};
use crate::clock::UnixMillis;
use crate::foods::{
    CustomFoodInput, CustomFoodStore, FoodId, FoodLimit, FoodStoreError, StoredFood,
};
use crate::runtime::Database;

impl CustomFoodStore for Database {
    type Error = FoodStoreError;

    async fn create_custom_food(&self, input: &CustomFoodInput) -> Result<StoredFood, Self::Error> {
        let operation: Result<StoredFood, sqlx::Error> = async {
        let now: UnixMillis = self.clock.now()?;
        let parsed_data = serde_json::to_string(&input.parsed_data)
            .map_err(|source| sqlx::Error::Encode(Box::new(source)))?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO crumbs_sources (id, created_at) VALUES ('custom', ?)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query_as::<_, StoredFoodRow>(
            "INSERT INTO crumbs_foods (
                source, source_food_id, dataset, description, category, notes, parsed_data, created_at
            ) VALUES ('custom', lower(hex(randomblob(16))), 'custom', ?, ?, ?, ?, ?)
            RETURNING id, source, source_food_id, dataset, description, category, notes, parsed_data",
        )
        .bind(input.description.as_str())
        .bind(input.category.as_deref())
        .bind(input.notes.as_deref())
        .bind(parsed_data)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await?;
        let food = row.into_result()?;
        transaction.commit().await?;
        Ok(food)

        }.await;
        operation.map_err(FoodStoreError::new)
    }

    async fn custom_food(&self, id: FoodId) -> Result<Option<StoredFood>, Self::Error> {
        let operation: Result<Option<StoredFood>, sqlx::Error> = async {
            let row = sqlx::query_as::<_, StoredFoodRow>(
            "SELECT id, source, source_food_id, dataset, description, category, notes, parsed_data
             FROM crumbs_foods
             WHERE id = ? AND source = 'custom' AND dataset = 'custom'",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
            row.map(StoredFoodRow::into_result).transpose()
        }
        .await;
        operation.map_err(FoodStoreError::new)
    }

    async fn list_custom_foods(&self, limit: FoodLimit) -> Result<Vec<StoredFood>, Self::Error> {
        let operation: Result<Vec<StoredFood>, sqlx::Error> = async {
            sqlx::query_as::<_, StoredFoodRow>(
            "SELECT id, source, source_food_id, dataset, description, category, notes, parsed_data
             FROM crumbs_foods
             WHERE source = 'custom' AND dataset = 'custom'
             ORDER BY id ASC
             LIMIT ?",
        )
        .bind(limit.get())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(StoredFoodRow::into_result)
        .collect()
        }
        .await;
        operation.map_err(FoodStoreError::new)
    }

    async fn update_custom_food(
        &self,
        id: FoodId,
        input: &CustomFoodInput,
    ) -> Result<Option<StoredFood>, Self::Error> {
        let operation: Result<Option<StoredFood>, sqlx::Error> = async {
        let now: UnixMillis = self.clock.now()?;
        let parsed_data = serde_json::to_string(&input.parsed_data)
            .map_err(|source| sqlx::Error::Encode(Box::new(source)))?;
        let row = sqlx::query_as::<_, StoredFoodRow>(
            "UPDATE crumbs_foods
             SET description = ?, category = ?, notes = ?, parsed_data = ?, updated_at = ?
             WHERE id = ? AND source = 'custom' AND dataset = 'custom'
             RETURNING id, source, source_food_id, dataset, description, category, notes, parsed_data",
        )
        .bind(input.description.as_str())
        .bind(input.category.as_deref())
        .bind(input.notes.as_deref())
        .bind(parsed_data)
        .bind(now)
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
        row.map(StoredFoodRow::into_result).transpose()

        }.await;
        operation.map_err(FoodStoreError::new)
    }

    async fn delete_custom_food(&self, id: FoodId) -> Result<bool, Self::Error> {
        let operation: Result<bool, sqlx::Error> = async {
            let deleted = sqlx::query_scalar::<_, RowId>(
                "DELETE FROM crumbs_foods
             WHERE id = ? AND source = 'custom' AND dataset = 'custom'
             RETURNING id",
            )
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;
            Ok(deleted.is_some())
        }
        .await;
        operation.map_err(FoodStoreError::new)
    }
}

#[cfg(test)]
#[path = "../../../unit-tests/foods/sqlite/custom_foods.rs"]
mod tests;
