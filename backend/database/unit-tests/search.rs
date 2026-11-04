use std::borrow::Cow;
use std::error::Error as StdError;

use crate::nutrition::{FoodData, NutritionBasis, NutritionProfile};
use crate::{
    Database, DatabaseConfig, Food, FoodId, FoodSearchResult, FoodWriter, SearchError, SearchLimit,
    SearchQuery, SearchQueryError, TagName,
};

#[tokio::test]
async fn searches_indexed_fields_and_returns_full_nutrition() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let mut food = make_food("usda", "1", "Roșii coapte");
    let mut nutrition = NutritionProfile::empty(NutritionBasis::Per100Grams);
    nutrition.energy = Some("18 kcal".parse()?);
    nutrition.protein = Some("0.9 g".parse()?);
    food.parsed_data = FoodData {
        nutrition: Some(nutrition),
        serving_sizes: Vec::new(),
    };
    let id = database.upsert_food(&food).await?;
    database
        .set_food_tags(id, &[TagName::new("seasonal")?])
        .await?;

    for expression in [
        "roșii",
        "category:Produce",
        "tags:seasonal",
        "\"roșii coapte\"",
    ] {
        let results = search(&database, expression, SearchLimit::default()).await?;
        let result = results
            .first()
            .ok_or_else(|| std::io::Error::other("expected a food search result"))?;
        assert_eq!(results.len(), 1, "MATCH {expression:?}");
        assert_eq!(result.id, id);
        assert_eq!(result.food.parsed_data, food.parsed_data);
        let json = serde_json::to_value(result)?;
        assert_eq!(json["parsed_data"]["nutrition"]["energy"], "18 kcal");
        assert_eq!(json["parsed_data"]["nutrition"]["protein"], "0.9 g");
    }
    Ok(())
}

#[tokio::test]
async fn supports_boolean_prefix_and_diacritic_matching() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let tomato = database
        .upsert_food(&make_food("usda", "1", "Roșii ripe tomato"))
        .await?;
    let potato = database
        .upsert_food(&make_food("usda", "2", "Roșii potato"))
        .await?;

    let mut matching_ids = search_ids(&database, "tomat* OR potato").await?;
    matching_ids.sort_by_key(|id| id.0);
    assert_eq!(matching_ids, [tomato, potato]);
    assert_eq!(search_ids(&database, "rosii AND potato").await?, [potato]);
    Ok(())
}

#[tokio::test]
async fn ranks_results_then_breaks_ties_by_food_id() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let longer = database
        .upsert_food(&make_food(
            "usda",
            "1",
            "Chicken with long grain brown rice",
        ))
        .await?;
    let shorter = database
        .upsert_food(&make_food("usda", "2", "Chicken soup"))
        .await?;
    let tie_first = database
        .upsert_food(&make_food("usda", "3", "Chicken bread"))
        .await?;
    let tie_second = database
        .upsert_food(&make_food("usda", "4", "Chicken cream"))
        .await?;

    assert_eq!(
        search_ids(&database, "chicken").await?,
        [shorter, tie_first, tie_second, longer]
    );
    Ok(())
}

#[tokio::test]
async fn search_returns_empty_for_unmatched_valid_expression() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    database
        .upsert_food(&make_food("usda", "1", "Plain rice"))
        .await?;
    assert!(
        search(&database, "unicorn", SearchLimit::default())
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn query_construction_and_deserialization_reject_empty_or_nul_values()
-> Result<(), Box<dyn StdError>> {
    assert_eq!(SearchQuery::new("  "), Err(SearchQueryError::Empty));
    assert_eq!(SearchQuery::new("milk\0cheese"), Err(SearchQueryError::Nul));
    assert!(serde_json::from_str::<SearchQuery>("\" \"").is_err());
    assert!(serde_json::from_str::<SearchQuery>("\"milk\\u0000cheese\"").is_err());
    Ok(())
}

#[tokio::test]
async fn malformed_and_unknown_column_expressions_are_invalid_queries()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    for expression in [
        "(milk OR cheese",
        "\"unterminated",
        "unknown:milk",
        "NEAR(milk cheese, nope)",
    ] {
        let error = database
            .search_foods(&SearchQuery::new(expression)?, SearchLimit::default())
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("expected invalid FTS5 expression error"))?;
        assert!(matches!(error, SearchError::InvalidQuery(_)), "{error}");
    }
    Ok(())
}

#[tokio::test]
async fn missing_search_column_is_reported_as_storage_failure() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    database
        .upsert_food(&make_food("usda", "1", "Milk"))
        .await?;
    sqlx::query("ALTER TABLE crumbs_foods RENAME COLUMN source TO source_broken")
        .execute(&database.pool)
        .await?;

    let error = database
        .search_foods(&SearchQuery::new("milk")?, SearchLimit::default())
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("expected a storage failure"))?;
    assert!(matches!(error, SearchError::Storage(_)), "{error}");
    Ok(())
}

#[tokio::test]
async fn missing_search_index_is_reported_as_storage_failure() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    sqlx::query("DROP TABLE crumbs_foods_fts")
        .execute(&database.pool)
        .await?;

    let error = database
        .search_foods(&SearchQuery::new("milk")?, SearchLimit::default())
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("expected a storage failure"))?;
    assert!(matches!(error, SearchError::Storage(_)), "{error}");
    Ok(())
}

#[tokio::test]
async fn bound_query_cannot_execute_sql_statements() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    database
        .upsert_food(&make_food("usda", "1", "Milk"))
        .await?;
    let expression = "milk'; DROP TABLE crumbs_foods; --";
    let _ = database
        .search_foods(&SearchQuery::new(expression)?, SearchLimit::default())
        .await;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM crumbs_foods")
        .fetch_one(&database.pool)
        .await?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn search_limits_default_and_clamp_to_the_contract_range() -> Result<(), Box<dyn StdError>> {
    assert_eq!(SearchLimit::default().get(), 20);
    for (input, expected) in [
        (i64::MIN, 20),
        (-1, 20),
        (0, 20),
        (1, 20),
        (19, 20),
        (20, 20),
        (1_000, 1_000),
        (1_001, 1_000),
        (i64::MAX, 1_000),
    ] {
        assert_eq!(SearchLimit::from(input).get(), expected, "input {input}");
    }
    for (json, expected) in [("1", 20), ("20", 20), ("1001", 1_000)] {
        let decoded: SearchLimit = serde_json::from_str(json)?;
        assert_eq!(decoded.get(), expected, "input {json}");
    }
    Ok(())
}

#[tokio::test]
async fn applies_limit_after_relevance_ranking() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    for index in 0..24 {
        let description = format!("chicken food {index}");
        database
            .upsert_food(&make_food("usda", &index.to_string(), &description))
            .await?;
    }
    let results = search(&database, "chicken", SearchLimit::from(1)).await?;
    assert_eq!(results.len(), 20);
    Ok(())
}

async fn open_database() -> Result<(tempfile::TempDir, Database), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
    Ok((directory, database))
}

fn make_food(source: &str, source_food_id: &str, description: &str) -> Food {
    Food {
        source: Cow::Owned(source.to_owned()),
        source_food_id: source_food_id.to_owned(),
        food_type: Cow::Borrowed("Foundation"),
        description: description.to_owned(),
        category: Some("Produce".to_owned()),
        parsed_data: FoodData::default(),
    }
}

async fn search(
    database: &Database,
    expression: &str,
    limit: SearchLimit,
) -> Result<Vec<FoodSearchResult>, Box<dyn StdError>> {
    Ok(database
        .search_foods(&SearchQuery::new(expression)?, limit)
        .await?)
}

async fn search_ids(
    database: &Database,
    expression: &str,
) -> Result<Vec<FoodId>, Box<dyn StdError>> {
    Ok(search(database, expression, SearchLimit::default())
        .await?
        .into_iter()
        .map(|result| result.id)
        .collect())
}
