use std::error::Error as StdError;
use std::io;
use std::time::{Duration, SystemTime};

use sqlx::{FromRow, query_scalar};

use crate::clock::{Clock, UnixMillis};
use crate::foods::*;
use crate::{Database, DatabaseConfig};

#[derive(Debug, PartialEq, Eq, FromRow)]
pub(super) struct Timestamps {
    pub(super) created_at: UnixMillis,
    pub(super) updated_at: Option<UnixMillis>,
}

pub(super) async fn open_database() -> Result<(tempfile::TempDir, Database), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
    Ok((directory, database))
}

pub(super) fn set_clock(database: &mut Database, time: SystemTime) {
    database.clock = Clock::with_source(move || time);
}

pub(super) fn at(millis: u64) -> Result<SystemTime, io::Error> {
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_millis(millis))
        .ok_or_else(|| io::Error::other("system time range does not cover the test timestamp"))
}

pub(super) fn before_epoch() -> Result<SystemTime, io::Error> {
    SystemTime::UNIX_EPOCH
        .checked_sub(Duration::from_millis(1))
        .ok_or_else(|| io::Error::other("system time range does not cover pre-epoch instants"))
}

pub(super) fn after_i64_millis() -> Result<SystemTime, io::Error> {
    // One millisecond past the largest signed 64-bit millisecond value.
    const OVERFLOW_MILLIS: u64 = 1 << 63;

    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_millis(OVERFLOW_MILLIS))
        .ok_or_else(|| io::Error::other("system time range does not cover the overflow timestamp"))
}

pub(super) fn make_food(
    source: &'static str,
    source_food_id: &str,
    description: &str,
) -> Result<Food, Box<dyn StdError>> {
    Ok(Food {
        source: Source::Borrowed(source),
        source_food_id: source_food_id.to_owned(),
        dataset: crate::foods::Dataset::Common,
        description: description.to_owned(),
        category: None,
        notes: None,
        parsed_data: crate::nutrition_model::FoodData::default(),
    })
}

pub(super) async fn source_timestamps(
    database: &Database,
    source: &str,
) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>(
        "SELECT created_at, updated_at FROM crumbs_sources WHERE id = ?",
    )
    .bind(source)
    .fetch_one(&database.pool)
    .await
}

pub(super) async fn food_timestamps(
    database: &Database,
    food: FoodId,
) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>("SELECT created_at, updated_at FROM crumbs_foods WHERE id = ?")
        .bind(food.0)
        .fetch_one(&database.pool)
        .await
}

pub(super) async fn tag_timestamps(
    database: &Database,
    tag: &TagName,
) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>(
        "SELECT created_at, updated_at FROM crumbs_tags WHERE name_key = ?",
    )
    .bind(tag.key())
    .fetch_one(&database.pool)
    .await
}

pub(super) async fn assignment_timestamps(
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
    .bind(tag.key())
    .fetch_one(&database.pool)
    .await
}

pub(super) async fn has_assignment(
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
    .bind(tag.key())
    .fetch_one(&database.pool)
    .await
}

pub(super) async fn find_tag(
    database: &Database,
    name: &TagName,
) -> Result<Tag, Box<dyn StdError>> {
    database
        .list_tags()
        .await?
        .into_iter()
        .find(|tag| tag.name == *name)
        .ok_or_else(|| io::Error::other(format!("missing tag {}", name.as_str())).into())
}

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
            matches!(
                rejected
                    .as_ref()
                    .err()
                    .and_then(|error| error.source())
                    .and_then(|source| source.downcast_ref::<sqlx::Error>()),
                Some(sqlx::Error::Encode(_))
            ),
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
            matches!(
                rejected
                    .as_ref()
                    .err()
                    .and_then(|error| error.source())
                    .and_then(|source| source.downcast_ref::<sqlx::Error>()),
                Some(sqlx::Error::Encode(_))
            ),
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
