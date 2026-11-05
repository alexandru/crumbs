mod common;

use std::borrow::Cow;
use std::error::Error as StdError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use common::DatabaseFixture;
use crumbs_database::{
    Database, Food, FoodCategory, FoodDescription, FoodId, FoodSummary, FoodWriter, Source, TagName,
};
use crumbs_import::import_usda;
use sqlx::{SqlitePool, query, query_scalar};

const FOUNDATION_JSON: &str = include_str!("../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../test-fixtures/branded.json");

type RowId = i64;
type FtsQuery<'a> = &'a str;

#[tokio::test]
async fn food_upserts_and_usda_reimports_preserve_tag_assignments() -> Result<(), Box<dyn StdError>>
{
    let fixture = DatabaseFixture::open().await?;
    let source = Source::Owned("usda".to_owned());
    let source_food_id = "321358";
    let initial = make_food(
        source.clone(),
        source_food_id,
        "Foundation",
        "Manual description",
        Some("Manual category"),
        r#"{"origin":"manual"}"#,
    )?;
    let initial_id = fixture.database.upsert_food(&initial).await?;
    let favorite = TagName::new("Favorite")?;
    fixture
        .database
        .set_food_tags(initial_id, std::slice::from_ref(&favorite))
        .await?;

    let update = make_food(
        source.clone(),
        source_food_id,
        "Foundation",
        "Updated manual description",
        Some("Updated manual category"),
        r#"{"origin":"updated"}"#,
    )?;
    let update_id = fixture.database.upsert_food(&update).await?;
    assert_eq!(update_id, initial_id);
    assert_eq!(
        food_ids_with_tag(&fixture.pool, favorite.as_str()).await?,
        vec![initial_id.0]
    );

    let path = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;
    let first_import = import_usda(&fixture.database, &path).await?;
    assert_eq!(first_import.committed().get(), 2);
    let imported = find_food(&fixture.database, &source, source_food_id).await?;
    assert_eq!(imported.id, initial_id);
    assert_eq!(imported.dataset.as_ref(), "usda_foundation");
    assert_eq!(imported.description, "Hummus, commercial");
    assert_eq!(
        imported.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert_eq!(
        food_ids_with_tag(&fixture.pool, favorite.as_str()).await?,
        vec![initial_id.0]
    );

    let second_import = import_usda(&fixture.database, &path).await?;
    assert_eq!(second_import.committed().get(), 2);
    let reimported = find_food(&fixture.database, &source, source_food_id).await?;
    assert_eq!(reimported.id, initial_id);
    assert_eq!(reimported.description, "Hummus, commercial");
    assert_eq!(
        food_ids_with_tag(&fixture.pool, favorite.as_str()).await?,
        vec![initial_id.0]
    );

    fixture.close().await;
    Ok(())
}

#[tokio::test]
async fn imported_fixtures_are_searchable_and_preserve_display_text()
-> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let foundation = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;
    let branded = write_input(fixture.directory.path(), "branded.json", BRANDED_JSON)?;

    import_usda(&fixture.database, &foundation).await?;
    import_usda(&fixture.database, &branded).await?;

    let hummus = food_id(&fixture.pool, "usda", "321358").await?;
    let tomatoes = food_id(&fixture.pool, "usda", "321360").await?;
    let granola = food_id(&fixture.pool, "usda", "1106281").await?;

    assert_matches(&fixture.pool, "hummus", &[hummus]).await?;
    assert_matches(&fixture.pool, "legumes", &[hummus]).await?;
    assert_matches(&fixture.pool, "category:legumes", &[hummus]).await?;
    assert_matches(&fixture.pool, "tomatoes", &[tomatoes]).await?;
    assert_matches(&fixture.pool, "vegetables", &[tomatoes]).await?;
    assert_matches(&fixture.pool, "description:tomatoes", &[tomatoes]).await?;
    assert_matches(&fixture.pool, "granola", &[granola]).await?;
    assert_matches(&fixture.pool, "cereal", &[granola]).await?;
    assert_eq!(
        description(&fixture.pool, hummus).await?,
        "Hummus, commercial"
    );
    assert_eq!(
        description(&fixture.pool, granola).await?,
        "GRANOLA, CINNAMON, RAISIN"
    );
    assert_eq!(
        category(&fixture.pool, hummus).await?.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert_eq!(
        category(&fixture.pool, granola).await?.as_deref(),
        Some("Cereal")
    );
    assert_fts_integrity(&fixture.pool).await?;
    Ok(())
}

#[tokio::test]
async fn reimport_replaces_stale_terms_without_duplicate_hits() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let foundation = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;
    import_usda(&fixture.database, &foundation).await?;
    let id = food_id(&fixture.pool, "usda", "321358").await?;
    let favorite = TagName::new("favorite")?;
    fixture
        .database
        .set_food_tags(FoodId(id), std::slice::from_ref(&favorite))
        .await?;
    assert_matches(&fixture.pool, "tags:favorite", &[id]).await?;

    let updated = write_input(
        fixture.directory.path(),
        "updated.json",
        r#"{"FoundationFoods":[{"fdcId":321358,"description":"Chickpea spread","dataType":"Foundation","foodCategory":{"description":"Spreads"}}]}"#,
    )?;
    import_usda(&fixture.database, &updated).await?;

    assert_eq!(food_id(&fixture.pool, "usda", "321358").await?, id);
    assert_eq!(description(&fixture.pool, id).await?, "Chickpea spread");
    for stale in ["hummus", "commercial", "legumes"] {
        assert_matches(&fixture.pool, stale, &[]).await?;
    }
    assert_matches(&fixture.pool, "chickpea", &[id]).await?;
    assert_matches(&fixture.pool, "spreads", &[id]).await?;
    assert_matches(&fixture.pool, "tags:favorite", &[id]).await?;
    assert_eq!(fts_row_count(&fixture.pool).await?, 2);

    for _ in 0..3 {
        import_usda(&fixture.database, &updated).await?;
    }
    assert_matches(&fixture.pool, "chickpea", &[id]).await?;
    assert_matches(&fixture.pool, "tags:favorite", &[id]).await?;
    assert_eq!(fts_row_count(&fixture.pool).await?, 2);
    assert_fts_integrity(&fixture.pool).await?;
    Ok(())
}

#[tokio::test]
async fn import_failure_keeps_earlier_food_searchable_and_retry_succeeds()
-> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(
        fixture.directory.path(),
        "partial.json",
        r#"{"FoundationFoods":[
            {"fdcId":9401,"description":"Searchable first food","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-1}]},
            {"fdcId":9402,"description":"Malformed food","dataType":"Foundation",}
        ]}"#,
    )?;

    let error = import_usda(&fixture.database, &path)
        .await
        .err()
        .ok_or_else(|| io::Error::other("malformed record unexpectedly imported"))?;
    assert_eq!(error.report().committed().get(), 1);
    assert_eq!(error.report().sanitized().get(), 1);
    assert_eq!(error.report().rejected().get(), 0);
    let first = food_id(&fixture.pool, "usda", "9401").await?;
    assert_matches(&fixture.pool, "searchable", &[first]).await?;
    assert_matches(&fixture.pool, "second", &[]).await?;

    fs::write(
        &path,
        r#"{"FoundationFoods":[
            {"fdcId":9401,"description":"Searchable first food","dataType":"Foundation"},
            {"fdcId":9402,"description":"Second food","dataType":"Foundation"}
        ]}"#,
    )?;
    import_usda(&fixture.database, &path).await?;
    let second = food_id(&fixture.pool, "usda", "9402").await?;
    assert_matches(&fixture.pool, "searchable", &[first]).await?;
    assert_matches(&fixture.pool, "second", &[second]).await?;
    assert_fts_integrity(&fixture.pool).await?;
    Ok(())
}

fn make_food(
    source: Source,
    source_food_id: &str,
    dataset: &'static str,
    description: &str,
    category: Option<&str>,
    parsed_data: &str,
) -> Result<Food, Box<dyn StdError>> {
    Ok(Food {
        source,
        source_food_id: source_food_id.to_owned(),
        dataset: Cow::Borrowed(dataset),
        description: description.to_owned(),
        category: category.map(str::to_owned),
        parsed_data: serde_json::from_str(parsed_data)?,
    })
}

async fn find_food(
    database: &Database,
    source: &Source,
    source_food_id: &str,
) -> Result<FoodSummary, Box<dyn StdError>> {
    let source_food_id = source_food_id.to_owned();
    match database.food_by_source(source, &source_food_id).await? {
        Some(food) => Ok(food),
        None => Err(io::Error::other(format!(
            "missing food {}/{}",
            source.as_ref(),
            source_food_id
        ))
        .into()),
    }
}

fn write_input(directory: &Path, name: &str, contents: &str) -> Result<PathBuf, Box<dyn StdError>> {
    let path = directory.join(name);
    fs::write(&path, contents)?;
    Ok(path)
}

async fn food_id(
    pool: &SqlitePool,
    source: &str,
    source_food_id: &str,
) -> Result<RowId, sqlx::Error> {
    query_scalar("SELECT id FROM crumbs_foods WHERE source = ? AND source_food_id = ?")
        .bind(source)
        .bind(source_food_id)
        .fetch_one(pool)
        .await
}

async fn description(pool: &SqlitePool, id: RowId) -> Result<FoodDescription, sqlx::Error> {
    query_scalar("SELECT description FROM crumbs_foods WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
}

async fn category(pool: &SqlitePool, id: RowId) -> Result<Option<FoodCategory>, sqlx::Error> {
    query_scalar("SELECT category FROM crumbs_foods WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
}

async fn food_ids_with_tag(pool: &SqlitePool, name: &str) -> Result<Vec<RowId>, sqlx::Error> {
    query_scalar(
        "SELECT ft.food_id FROM crumbs_food_tags ft
         JOIN crumbs_tags t ON t.id = ft.tag_id
         WHERE t.name = ? ORDER BY ft.food_id",
    )
    .bind(name)
    .fetch_all(pool)
    .await
}

async fn matching_ids(
    pool: &SqlitePool,
    query_text: FtsQuery<'_>,
) -> Result<Vec<RowId>, sqlx::Error> {
    query_scalar(
        "SELECT f.id
         FROM crumbs_foods_fts
         JOIN crumbs_foods AS f ON f.id = crumbs_foods_fts.rowid
         WHERE crumbs_foods_fts MATCH ?
         ORDER BY f.id",
    )
    .bind(query_text)
    .fetch_all(pool)
    .await
}

async fn assert_matches(
    pool: &SqlitePool,
    query_text: FtsQuery<'_>,
    expected: &[RowId],
) -> Result<(), Box<dyn StdError>> {
    let actual = matching_ids(pool, query_text).await?;
    assert_eq!(actual, expected, "MATCH {query_text:?}");
    Ok(())
}

async fn fts_row_count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM crumbs_foods_fts")
        .fetch_one(pool)
        .await
}

async fn assert_fts_integrity(pool: &SqlitePool) -> Result<(), Box<dyn StdError>> {
    query(
        "INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rank)
         VALUES ('integrity-check', 1)",
    )
    .execute(pool)
    .await?;
    let integrity: String = query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?;
    assert_eq!(integrity, "ok");
    Ok(())
}
