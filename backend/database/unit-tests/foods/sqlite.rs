use std::borrow::Cow;
use std::error::Error as StdError;
use std::io;
use std::time::{Duration, SystemTime};

use sqlx::{FromRow, query_scalar};

use crate::clock::{Clock, UnixMillis};
use crate::{
    Database, DatabaseConfig, Food, FoodId, FoodReader, FoodSearchResult, FoodWriter, SearchError,
    SearchLimit, SearchQuery, Source, Tag, TagName,
};

#[tokio::test]
async fn inserts_application_timestamps_and_leaves_updates_empty() -> Result<(), Box<dyn StdError>>
{
    let (_directory, mut database) = open_database().await?;
    set_clock(&mut database, at(1_000)?);
    let food_id = database
        .upsert_food(&make_food("usda", "1", "Inserted food")?)
        .await?;
    let tag = TagName::new("Fresh")?;
    set_clock(&mut database, at(1_500)?);
    database
        .set_food_tags(food_id, std::slice::from_ref(&tag))
        .await?;

    let inserted = Timestamps {
        created_at: 1_000,
        updated_at: None,
    };
    assert_eq!(source_timestamps(&database, "usda").await?, inserted);
    assert_eq!(food_timestamps(&database, food_id).await?, inserted);
    let tagged = Timestamps {
        created_at: 1_500,
        updated_at: None,
    };
    assert_eq!(tag_timestamps(&database, &tag).await?, tagged);
    assert_eq!(
        assignment_timestamps(&database, food_id, &tag).await?,
        tagged
    );

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn food_updates_preserve_created_at_and_accept_backwards_clock()
-> Result<(), Box<dyn StdError>> {
    let (_directory, mut database) = open_database().await?;
    set_clock(&mut database, at(2_000)?);
    let initial_id = database
        .upsert_food(&make_food("usda", "2", "Initial food")?)
        .await?;

    set_clock(&mut database, at(1_500)?);
    let updated_id = database
        .upsert_food(&make_food("usda", "2", "Updated food")?)
        .await?;

    assert_eq!(updated_id, initial_id);
    assert_eq!(
        food_timestamps(&database, initial_id).await?,
        Timestamps {
            created_at: 2_000,
            updated_at: Some(1_500),
        }
    );
    assert_eq!(
        source_timestamps(&database, "usda").await?,
        Timestamps {
            created_at: 2_000,
            updated_at: None,
        }
    );
    let summary = database
        .food_by_source(&Source::Borrowed("usda"), &"2".to_owned())
        .await?
        .ok_or_else(|| io::Error::other("updated food is missing"))?;
    assert_eq!(summary.description, "Updated food");

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn retained_assignments_keep_created_at_while_removed_ones_are_recreated()
-> Result<(), Box<dyn StdError>> {
    let (_directory, mut database) = open_database().await?;
    set_clock(&mut database, at(3_000)?);
    let food_id = database
        .upsert_food(&make_food("community", "3", "Tagged food")?)
        .await?;
    let favorite = TagName::new("Favorite")?;
    let seasonal = TagName::new("Seasonal")?;
    database
        .set_food_tags(food_id, &[favorite.clone(), seasonal.clone()])
        .await?;

    set_clock(&mut database, at(3_500)?);
    let fresh = TagName::new("Fresh")?;
    database
        .set_food_tags(food_id, &[favorite.clone(), fresh.clone()])
        .await?;

    let retained = Timestamps {
        created_at: 3_000,
        updated_at: None,
    };
    assert_eq!(
        assignment_timestamps(&database, food_id, &favorite).await?,
        retained
    );
    assert_eq!(
        assignment_timestamps(&database, food_id, &fresh).await?,
        Timestamps {
            created_at: 3_500,
            updated_at: None,
        }
    );
    assert!(
        !has_assignment(&database, food_id, &seasonal).await?,
        "removed assignment was retained"
    );
    assert_eq!(
        tag_timestamps(&database, &seasonal).await?,
        retained,
        "removing an assignment changed the catalog timestamp"
    );

    set_clock(&mut database, at(4_000)?);
    database
        .set_food_tags(food_id, std::slice::from_ref(&seasonal))
        .await?;

    assert!(
        !has_assignment(&database, food_id, &favorite).await?,
        "unrequested assignment was retained"
    );
    assert_eq!(
        assignment_timestamps(&database, food_id, &seasonal).await?,
        Timestamps {
            created_at: 4_000,
            updated_at: None,
        },
        "re-added assignment reused its removed row"
    );
    assert_eq!(
        tag_timestamps(&database, &seasonal).await?,
        retained,
        "re-adding an assignment changed the catalog timestamp"
    );

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn catalog_conflicts_keep_first_spelling_and_timestamps() -> Result<(), Box<dyn StdError>> {
    let (_directory, mut database) = open_database().await?;
    set_clock(&mut database, at(5_000)?);
    let food_id = database
        .upsert_food(&make_food("community", "4", "Catalog food")?)
        .await?;
    let vegan = TagName::new("Vegan")?;
    database
        .set_food_tags(food_id, std::slice::from_ref(&vegan))
        .await?;

    set_clock(&mut database, at(6_000)?);
    let duplicate = TagName::new("vegan")?;
    database
        .set_food_tags(food_id, std::slice::from_ref(&duplicate))
        .await?;

    let tags = database.list_tags().await?;
    let stored = tags
        .first()
        .ok_or_else(|| io::Error::other("tag catalog is empty"))?;
    assert_eq!(tags.len(), 1);
    assert_eq!(stored.name.as_str(), "Vegan");
    let catalogued = Timestamps {
        created_at: 5_000,
        updated_at: None,
    };
    assert_eq!(tag_timestamps(&database, &vegan).await?, catalogued);
    assert_eq!(
        assignment_timestamps(&database, food_id, &vegan).await?,
        catalogued
    );

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_clocks_are_rejected_before_food_and_tag_writes() -> Result<(), Box<dyn StdError>> {
    let invalid_clocks = [
        ("pre-epoch", before_epoch()?),
        ("out-of-range", after_i64_millis()?),
    ];

    for (label, invalid) in invalid_clocks {
        let (_directory, mut database) = open_database().await?;
        set_clock(&mut database, invalid);
        let rejected = database
            .upsert_food(&make_food("usda", "5", "Rejected food")?)
            .await;
        assert!(
            matches!(rejected, Err(sqlx::Error::Encode(_))),
            "{label} clock did not fail the food insert as an encode error"
        );
        let sources: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_sources")
            .fetch_one(&database.pool)
            .await?;
        let foods: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_foods")
            .fetch_one(&database.pool)
            .await?;
        assert_eq!(sources, 0, "{label} clock wrote a source row");
        assert_eq!(foods, 0, "{label} clock wrote a food row");

        set_clock(&mut database, at(1_000)?);
        let food_id = database
            .upsert_food(&make_food("usda", "5", "Accepted food")?)
            .await?;
        set_clock(&mut database, invalid);
        let tag = TagName::new("Rejected")?;
        let rejected = database
            .set_food_tags(food_id, std::slice::from_ref(&tag))
            .await;
        assert!(
            matches!(rejected, Err(sqlx::Error::Encode(_))),
            "{label} clock did not fail tag replacement as an encode error"
        );
        let tags: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_tags")
            .fetch_one(&database.pool)
            .await?;
        let assignments: i64 = query_scalar("SELECT COUNT(*) FROM crumbs_food_tags")
            .fetch_one(&database.pool)
            .await?;
        assert_eq!(tags, 0, "{label} clock catalogued a tag");
        assert_eq!(assignments, 0, "{label} clock assigned a tag");
        assert_eq!(
            food_timestamps(&database, food_id).await?,
            Timestamps {
                created_at: 1_000,
                updated_at: None,
            }
        );

        database.close().await;
    }

    Ok(())
}

#[tokio::test]
async fn autoincrement_ids_are_not_reused_for_foods_and_tags() -> Result<(), Box<dyn StdError>> {
    let (_directory, mut database) = open_database().await?;
    set_clock(&mut database, at(7_000)?);

    let food_schema: String = query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'crumbs_foods'",
    )
    .fetch_one(&database.pool)
    .await?;
    let tag_schema: String =
        query_scalar("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'crumbs_tags'")
            .fetch_one(&database.pool)
            .await?;
    assert!(food_schema.contains("AUTOINCREMENT"));
    assert!(tag_schema.contains("AUTOINCREMENT"));

    let first = database
        .upsert_food(&make_food("usda", "6", "First food")?)
        .await?;
    let second = database
        .upsert_food(&make_food("usda", "7", "Second food")?)
        .await?;
    assert!(second.0 > first.0);
    sqlx::query("DELETE FROM crumbs_foods WHERE id = ?")
        .bind(second.0)
        .execute(&database.pool)
        .await?;
    let third = database
        .upsert_food(&make_food("usda", "8", "Third food")?)
        .await?;
    assert!(
        third.0 > second.0,
        "deleted food id {} was reused by {}",
        second.0,
        third.0
    );

    let alpha = TagName::new("Alpha")?;
    let beta = TagName::new("Beta")?;
    let gamma = TagName::new("Gamma")?;
    database
        .set_food_tags(first, std::slice::from_ref(&alpha))
        .await?;
    let alpha_tag = find_tag(&database, &alpha).await?;
    database
        .set_food_tags(first, std::slice::from_ref(&beta))
        .await?;
    let beta_tag = find_tag(&database, &beta).await?;
    assert!(beta_tag.id.0 > alpha_tag.id.0);
    sqlx::query("DELETE FROM crumbs_tags WHERE id = ?")
        .bind(beta_tag.id.0)
        .execute(&database.pool)
        .await?;
    database
        .set_food_tags(first, std::slice::from_ref(&gamma))
        .await?;
    let gamma_tag = find_tag(&database, &gamma).await?;
    assert!(
        gamma_tag.id.0 > beta_tag.id.0,
        "deleted tag id {} was reused by {}",
        beta_tag.id.0,
        gamma_tag.id.0
    );

    database.close().await;
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

fn set_clock(database: &mut Database, time: SystemTime) {
    database.clock = Clock::with_source(move || time);
}

fn at(millis: u64) -> Result<SystemTime, io::Error> {
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_millis(millis))
        .ok_or_else(|| io::Error::other("system time range does not cover the test timestamp"))
}

fn before_epoch() -> Result<SystemTime, io::Error> {
    SystemTime::UNIX_EPOCH
        .checked_sub(Duration::from_millis(1))
        .ok_or_else(|| io::Error::other("system time range does not cover pre-epoch instants"))
}

fn after_i64_millis() -> Result<SystemTime, io::Error> {
    // One millisecond past the largest signed 64-bit millisecond value.
    const OVERFLOW_MILLIS: u64 = 1 << 63;

    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_millis(OVERFLOW_MILLIS))
        .ok_or_else(|| io::Error::other("system time range does not cover the overflow timestamp"))
}

fn make_food(
    source: &'static str,
    source_food_id: &str,
    description: &str,
) -> Result<Food, Box<dyn StdError>> {
    Ok(Food {
        source: Source::Borrowed(source),
        source_food_id: source_food_id.to_owned(),
        dataset: Cow::Borrowed("Foundation"),
        description: description.to_owned(),
        category: None,
        parsed_data: crate::nutrition::FoodData::default(),
    })
}

async fn source_timestamps(database: &Database, source: &str) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>(
        "SELECT created_at, updated_at FROM crumbs_sources WHERE id = ?",
    )
    .bind(source)
    .fetch_one(&database.pool)
    .await
}

async fn food_timestamps(database: &Database, food: FoodId) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>("SELECT created_at, updated_at FROM crumbs_foods WHERE id = ?")
        .bind(food.0)
        .fetch_one(&database.pool)
        .await
}

async fn tag_timestamps(database: &Database, tag: &TagName) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>(
        "SELECT created_at, updated_at FROM crumbs_tags WHERE name_key = ?",
    )
    .bind(&tag.key)
    .fetch_one(&database.pool)
    .await
}

async fn assignment_timestamps(
    database: &Database,
    food: FoodId,
    tag: &TagName,
) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>(
        "SELECT ft.created_at, ft.updated_at
         FROM crumbs_food_tags AS ft
         JOIN crumbs_tags AS t ON t.id = ft.tag_id
         WHERE ft.food_id = ? AND t.name_key = ?",
    )
    .bind(food.0)
    .bind(&tag.key)
    .fetch_one(&database.pool)
    .await
}

async fn has_assignment(
    database: &Database,
    food: FoodId,
    tag: &TagName,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM crumbs_food_tags AS ft
             JOIN crumbs_tags AS t ON t.id = ft.tag_id
             WHERE ft.food_id = ? AND t.name_key = ?
         )",
    )
    .bind(food.0)
    .bind(&tag.key)
    .fetch_one(&database.pool)
    .await
}

async fn find_tag(database: &Database, name: &TagName) -> Result<Tag, Box<dyn StdError>> {
    database
        .list_tags()
        .await?
        .into_iter()
        .find(|tag| tag.name == *name)
        .ok_or_else(|| io::Error::other(format!("missing tag {}", name.as_str())).into())
}

#[derive(Debug, PartialEq, Eq, FromRow)]
struct Timestamps {
    created_at: UnixMillis,
    updated_at: Option<UnixMillis>,
}

#[tokio::test]
async fn searches_indexed_fields_and_returns_full_nutrition() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let mut food = make_food("usda", "1", "Roșii coapte")?;
    food.category = Some("Produce".to_owned());
    let mut nutrition =
        crate::nutrition::NutritionProfile::empty(crate::nutrition::NutritionBasis::Per100Grams);
    nutrition.energy = Some("18 kcal".parse()?);
    nutrition.protein = Some("0.9 g".parse()?);
    food.parsed_data = crate::nutrition::FoodData {
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
        let results = search_database(&database, expression, SearchLimit::default()).await?;
        let result = results
            .first()
            .ok_or_else(|| io::Error::other("expected a food search result"))?;
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
        .upsert_food(&make_food("usda", "1", "Roșii ripe tomato")?)
        .await?;
    let potato = database
        .upsert_food(&make_food("usda", "2", "Roșii potato")?)
        .await?;

    let mut matching_ids = search_food_ids(&database, "tomat* OR potato").await?;
    matching_ids.sort_by_key(|id| id.0);
    assert_eq!(matching_ids, [tomato, potato]);
    assert_eq!(
        search_food_ids(&database, "rosii AND potato").await?,
        [potato]
    );
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
        )?)
        .await?;
    let shorter = database
        .upsert_food(&make_food("usda", "2", "Chicken soup")?)
        .await?;
    let tie_first = database
        .upsert_food(&make_food("usda", "3", "Chicken bread")?)
        .await?;
    let tie_second = database
        .upsert_food(&make_food("usda", "4", "Chicken cream")?)
        .await?;

    assert_eq!(
        search_food_ids(&database, "chicken").await?,
        [shorter, tie_first, tie_second, longer]
    );
    Ok(())
}

#[tokio::test]
async fn search_returns_empty_for_unmatched_valid_expression() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    database
        .upsert_food(&make_food("usda", "1", "Plain rice")?)
        .await?;
    assert!(
        search_database(&database, "unicorn", SearchLimit::default())
            .await?
            .is_empty()
    );
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
            .ok_or_else(|| io::Error::other("expected invalid FTS5 expression error"))?;
        assert!(matches!(error, SearchError::InvalidQuery(_)), "{error}");
    }
    Ok(())
}

#[tokio::test]
async fn missing_search_column_is_reported_as_storage_failure() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    database
        .upsert_food(&make_food("usda", "1", "Milk")?)
        .await?;
    sqlx::query("ALTER TABLE crumbs_foods RENAME COLUMN parsed_data TO parsed_data_broken")
        .execute(&database.pool)
        .await?;

    let error = database
        .search_foods(&SearchQuery::new("milk")?, SearchLimit::default())
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected a storage failure"))?;
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
        .ok_or_else(|| io::Error::other("expected a storage failure"))?;
    assert!(matches!(error, SearchError::Storage(_)), "{error}");
    Ok(())
}

#[tokio::test]
async fn bound_query_cannot_execute_sql_statements() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    database
        .upsert_food(&make_food("usda", "1", "Milk")?)
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

#[tokio::test]
async fn applies_limit_after_relevance_ranking() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    for index in 0..24 {
        let description = format!("chicken food {index}");
        database
            .upsert_food(&make_food("usda", &index.to_string(), &description)?)
            .await?;
    }
    assert_eq!(
        search_database(&database, "chicken", SearchLimit::from(1))
            .await?
            .len(),
        20
    );
    Ok(())
}

async fn search_database(
    database: &Database,
    expression: &str,
    limit: SearchLimit,
) -> Result<Vec<FoodSearchResult>, Box<dyn StdError>> {
    Ok(database
        .search_foods(&SearchQuery::new(expression)?, limit)
        .await?)
}

async fn search_food_ids(
    database: &Database,
    expression: &str,
) -> Result<Vec<FoodId>, Box<dyn StdError>> {
    Ok(
        search_database(database, expression, SearchLimit::default())
            .await?
            .into_iter()
            .map(|result| result.id)
            .collect(),
    )
}
