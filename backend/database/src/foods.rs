//! Persistence helpers for the `crumbs_foods` table.

use sqlx::{Sqlite, Transaction};

use crate::usda_import::PreparedFood;

pub(crate) type Source = &'static str;

pub(crate) const USDA_SOURCE: Source = "usda";

pub(crate) async fn upsert_usda_food(
    transaction: &mut Transaction<'_, Sqlite>,
    food: &PreparedFood,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO crumbs_foods (
            source,
            source_food_id,
            food_type,
            description,
            category,
            tags,
            source_data
        ) VALUES (?, ?, ?, ?, ?, '', ?)
        ON CONFLICT (source, source_food_id) DO UPDATE SET
            food_type = excluded.food_type,
            description = excluded.description,
            category = excluded.category,
            tags = excluded.tags,
            source_data = excluded.source_data",
    )
    .bind(USDA_SOURCE)
    .bind(food.fdc_id.as_str())
    .bind(food.kind.as_str())
    .bind(food.description.as_str())
    .bind(food.category.as_ref().map(|category| category.as_str()))
    .bind(food.raw.get())
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
