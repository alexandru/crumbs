use std::error::Error as StdError;
use std::io;

use super::*;
use crate::{Database, DatabaseConfig};

pub(super) async fn open_database() -> Result<(tempfile::TempDir, Database), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
    Ok((directory, database))
}

pub(super) fn make_food(
    source: Source,
    source_food_id: &str,
    dataset: Dataset,
    description: &str,
    category: Option<&str>,
    parsed_data: &str,
) -> Result<Food, Box<dyn StdError>> {
    Ok(Food {
        source,
        source_food_id: source_food_id.to_owned(),
        dataset,
        description: description.to_owned(),
        category: category.map(str::to_owned),
        notes: None,
        parsed_data: serde_json::from_str(parsed_data)?,
    })
}

pub(super) async fn find_food(
    database: &Database,
    source: &Source,
    source_food_id: &str,
) -> Result<FoodSummary, Box<dyn StdError>> {
    match database
        .food_by_source(source.as_ref(), source_food_id)
        .await?
    {
        Some(food) => Ok(food),
        None => Err(io::Error::other(format!(
            "missing food {}/{}",
            source.as_ref(),
            source_food_id
        ))
        .into()),
    }
}

pub(super) async fn ids_with_tag(
    database: &Database,
    tag: &TagName,
) -> Result<Vec<FoodId>, sqlx::Error> {
    type RowId = i64;

    Ok(sqlx::query_scalar::<_, RowId>(
        "SELECT ft.food_id FROM crumbs_food_tags ft
         JOIN crumbs_tags t ON t.id = ft.tag_id
         WHERE t.name_key = ? ORDER BY ft.food_id",
    )
    .bind(tag.key())
    .fetch_all(&database.pool)
    .await?
    .into_iter()
    .map(FoodId)
    .collect())
}
