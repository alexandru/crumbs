use std::borrow::Cow;
use std::error::Error as StdError;
use std::io;
use std::time::{Duration, SystemTime};

use serde_json::value::RawValue;
use sqlx::{FromRow, query_scalar};

use crate::clock::{Clock, UnixMillis};
use crate::{Database, DatabaseConfig, Food, FoodId, FoodWriter, Source, Tag, TagName};

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
    database.clock = Clock::controlled(move || time);
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
        food_type: Cow::Borrowed("Foundation"),
        description: description.to_owned(),
        category: None,
        source_data: serde_json::from_str::<Box<RawValue>>("{}")?,
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
