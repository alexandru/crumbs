use std::collections::HashSet;

use sqlx::Row;

use super::rows::TagAssignmentRow;
use crate::clock::UnixMillis;
use crate::foods::{FoodId, FoodStoreError, Tag, TagId, TagName, TagStore};
use crate::runtime::Database;

impl TagStore for Database {
    type Error = FoodStoreError;

    async fn set_food_tags(&self, food: FoodId, tags: &[TagName]) -> Result<(), Self::Error> {
        let operation: Result<(), sqlx::Error> = async {
            let mut transaction = self.pool.begin().await?;
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM crumbs_foods WHERE id = ?)")
                    .bind(food.0)
                    .fetch_one(&mut *transaction)
                    .await?;
            if !exists {
                return Err(sqlx::Error::RowNotFound);
            }
            let now: UnixMillis = self.clock.now()?;
            let assignments = sqlx::query_as::<_, TagAssignmentRow>(
                "SELECT t.id AS tag_id, t.name_key
             FROM crumbs_food_tags AS ft
             JOIN crumbs_tags AS t ON t.id = ft.tag_id
             WHERE ft.food_id = ?",
            )
            .bind(food.0)
            .fetch_all(&mut *transaction)
            .await?;
            let requested: HashSet<&str> = tags.iter().map(|tag| tag.key()).collect();
            for assignment in assignments {
                if !requested.contains(assignment.name_key.as_str()) {
                    sqlx::query("DELETE FROM crumbs_food_tags WHERE food_id = ? AND tag_id = ?")
                        .bind(food.0)
                        .bind(assignment.tag_id)
                        .execute(&mut *transaction)
                        .await?;
                }
            }
            for tag in tags {
                sqlx::query(
                    "INSERT INTO crumbs_tags (name, name_key, created_at) VALUES (?, ?, ?)
                 ON CONFLICT (name_key) DO NOTHING",
                )
                .bind(tag.as_str())
                .bind(tag.key())
                .bind(now)
                .execute(&mut *transaction)
                .await?;
                sqlx::query(
                    "INSERT INTO crumbs_food_tags (food_id, tag_id, created_at)
                 SELECT ?, id, ? FROM crumbs_tags WHERE name_key = ?
                 ON CONFLICT (food_id, tag_id) DO NOTHING",
                )
                .bind(food.0)
                .bind(now)
                .bind(tag.key())
                .execute(&mut *transaction)
                .await?;
            }
            transaction.commit().await
        }
        .await;
        operation.map_err(FoodStoreError::new)
    }

    async fn list_tags(&self) -> Result<Vec<Tag>, Self::Error> {
        let operation: Result<Vec<Tag>, sqlx::Error> = async {
            sqlx::query("SELECT id, name FROM crumbs_tags ORDER BY name_key")
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(|row| {
                    Ok(Tag {
                        id: TagId(row.try_get("id")?),
                        name: TagName::new(row.try_get::<String, _>("name")?)
                            .map_err(|source| sqlx::Error::Decode(Box::new(source)))?,
                    })
                })
                .collect()
        }
        .await;
        operation.map_err(FoodStoreError::new)
    }
}

#[cfg(test)]
#[path = "../../../unit-tests/foods/sqlite/tags.rs"]
mod tests;
