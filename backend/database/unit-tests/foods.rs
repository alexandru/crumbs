use std::borrow::Cow;
use std::error::Error as StdError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::*;
use crate::{Database, DatabaseConfig, ParsedFoodData};

const FOUNDATION_JSON: &str = include_str!("../test-fixtures/foundation.json");

#[tokio::test]
async fn upsert_foods_are_scoped_by_provider_and_keep_opaque_ids() -> Result<(), Box<dyn StdError>>
{
    let (_directory, database) = open_database().await?;
    let source = Source::Owned("regional catalog/β".to_owned());
    let source_food_id = "provider-record:food/ß".to_owned();
    let initial = make_food(
        source.clone(),
        &source_food_id,
        "External",
        "Original food",
        Some("Original category"),
        r#"{"nutrition":null,"serving_sizes":[{"name":"first","quantity":"10 g"}]}"#,
    )?;

    let first_id = database.upsert_food(&initial).await?;
    let updated = make_food(
        source.clone(),
        &source_food_id,
        "External",
        "Updated food",
        Some("Updated category"),
        r#"{"nutrition":null,"serving_sizes":[{"name":"second","quantity":"20 g"}]}"#,
    )?;
    let updated_id = database.upsert_food(&updated).await?;

    assert_eq!(updated_id, first_id);
    let summary = find_food(&database, &source, &source_food_id).await?;
    assert_eq!(summary.id, first_id);
    assert_eq!(summary.source_food_id, source_food_id);
    assert_eq!(summary.description, "Updated food");
    assert_eq!(summary.category.as_deref(), Some("Updated category"));

    let other_source = Source::Owned("another catalog".to_owned());
    let same_source_id = make_food(
        other_source.clone(),
        &source_food_id,
        "Remote",
        "Other provider food",
        None,
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    let other_id = database.upsert_food(&same_source_id).await?;
    assert_ne!(other_id, first_id);
    let other_summary = find_food(&database, &other_source, &source_food_id).await?;
    assert_eq!(other_summary.id, other_id);

    let leading_zero_id = "0000123456789";
    let leading_zero_food = make_food(
        source.clone(),
        leading_zero_id,
        "External",
        "Leading zero identifier",
        None,
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    let leading_zero_food_id = database.upsert_food(&leading_zero_food).await?;
    let leading_zero_summary = find_food(&database, &source, leading_zero_id).await?;
    assert_eq!(leading_zero_summary.id, leading_zero_food_id);
    assert_eq!(leading_zero_summary.source_food_id, leading_zero_id);

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn tag_membership_uses_casefolded_exact_names_and_keeps_first_spelling()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let food = make_food(
        Source::Borrowed("community"),
        "food-1",
        "Community",
        "Oat drink",
        None,
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    let food_id = database.upsert_food(&food).await?;
    let vegan = TagName::new(" Vegan ")?;
    let vegan_duplicate = TagName::new("vegan")?;
    let food_safety = TagName::new("Food Safety")?;

    database
        .set_food_tags(
            food_id,
            &[vegan.clone(), vegan_duplicate.clone(), food_safety],
        )
        .await?;

    let lowercase_vegan = TagName::new("vegan")?;
    assert_eq!(
        ids_with_tag(&database, &lowercase_vegan).await?,
        vec![food_id]
    );
    let partial_name = TagName::new("Food")?;
    assert!(ids_with_tag(&database, &partial_name).await?.is_empty());
    let unknown_name = TagName::new("unassigned")?;
    assert!(ids_with_tag(&database, &unknown_name).await?.is_empty());

    let tags = database.list_tags().await?;
    let names = tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["Food Safety", "Vegan"]);
    assert_eq!(vegan.as_str(), "Vegan");

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn tag_names_fold_unicode_case_without_removing_accents() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let food = make_food(
        Source::Borrowed("community"),
        "food-2",
        "Community",
        "Romanian food",
        None,
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    let food_id = database.upsert_food(&food).await?;
    let strasse = TagName::new("Straße")?;
    let romanian = TagName::new("Șuncă")?;
    let soft_s = TagName::new("Ș")?;
    let plain_s = TagName::new("s")?;
    let decomposed_uppercase = TagName::new("S\u{0326}UNCA\u{0306}")?;

    assert_eq!(romanian, decomposed_uppercase);
    assert_ne!(soft_s, plain_s);
    database
        .set_food_tags(food_id, &[strasse, romanian.clone(), soft_s])
        .await?;

    let uppercase_strasse = TagName::new("STRASSE")?;
    assert_eq!(
        ids_with_tag(&database, &uppercase_strasse).await?,
        vec![food_id]
    );
    assert_eq!(
        ids_with_tag(&database, &decomposed_uppercase).await?,
        vec![food_id]
    );
    assert!(ids_with_tag(&database, &plain_s).await?.is_empty());
    assert!(matches!(TagName::new(" \t\n"), Err(InvalidTagName)));

    database.close().await;
    Ok(())
}

#[test]
fn tag_names_fold_full_case_canonical_forms_and_latest_unicode_mappings()
-> Result<(), Box<dyn StdError>> {
    assert_eq!(TagName::new("Straße")?, TagName::new("STRASSE")?);
    assert_eq!(TagName::new("ẞ")?, TagName::new("ss")?);
    assert_eq!(TagName::new("Σ")?, TagName::new("σ")?);
    assert_eq!(TagName::new("ς")?, TagName::new("σ")?);
    assert_eq!(TagName::new("ﬃ")?, TagName::new("ffi")?);

    assert_eq!(TagName::new("é")?, TagName::new("e\u{0301}")?);
    assert_eq!(
        TagName::new("Șuncă")?,
        TagName::new("S\u{0326}UNCA\u{0306}")?
    );
    assert_ne!(TagName::new("é")?, TagName::new("e")?);
    assert_ne!(TagName::new("Ș")?, TagName::new("s")?);

    assert_eq!(TagName::new("\u{A7CE}")?, TagName::new("\u{A7CF}")?);
    assert_eq!(TagName::new("\u{16EA0}")?, TagName::new("\u{16EBB}")?);

    assert_eq!(TagName::new(" Straße ")?.as_str(), "Straße");
    Ok(())
}

#[tokio::test]
async fn food_upserts_and_usda_reimports_preserve_tag_assignments() -> Result<(), Box<dyn StdError>>
{
    let (directory, database) = open_database().await?;
    let source = Source::Borrowed("usda");
    let source_food_id = "321358";
    let initial = make_food(
        source.clone(),
        source_food_id,
        "Foundation",
        "Manual description",
        Some("Manual category"),
        r#"{"nutrition":null,"serving_sizes":[{"name":"manual","quantity":"10 g"}]}"#,
    )?;
    let initial_id = database.upsert_food(&initial).await?;
    let favorite = TagName::new("Favorite")?;
    database
        .set_food_tags(initial_id, std::slice::from_ref(&favorite))
        .await?;

    let update = make_food(
        source.clone(),
        source_food_id,
        "Foundation",
        "Updated manual description",
        Some("Updated manual category"),
        r#"{"nutrition":null,"serving_sizes":[{"name":"updated","quantity":"20 g"}]}"#,
    )?;
    let update_id = database.upsert_food(&update).await?;
    assert_eq!(update_id, initial_id);
    assert_eq!(ids_with_tag(&database, &favorite).await?, vec![initial_id]);

    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    let first_import = crate::import_usda(&database, &path).await?;
    assert_eq!(first_import.committed().get(), 2);
    let imported = find_food(&database, &source, source_food_id).await?;
    assert_eq!(imported.id, initial_id);
    assert_eq!(imported.food_type.as_ref(), "Foundation");
    assert_eq!(imported.description, "Hummus, commercial");
    assert_eq!(
        imported.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert_eq!(ids_with_tag(&database, &favorite).await?, vec![initial_id]);

    let second_import = crate::import_usda(&database, &path).await?;
    assert_eq!(second_import.committed().get(), 2);
    let reimported = find_food(&database, &source, source_food_id).await?;
    assert_eq!(reimported.id, initial_id);
    assert_eq!(reimported.description, "Hummus, commercial");
    assert_eq!(ids_with_tag(&database, &favorite).await?, vec![initial_id]);

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn clearing_assignments_keeps_catalog_and_unknown_food_does_not_change_it()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let food = make_food(
        Source::Borrowed("community"),
        "food-3",
        "Community",
        "Tagged food",
        None,
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    let food_id = database.upsert_food(&food).await?;
    let existing = TagName::new("Seasonal")?;
    database
        .set_food_tags(food_id, std::slice::from_ref(&existing))
        .await?;
    let catalog_before_clear = database.list_tags().await?;

    database.set_food_tags(food_id, &[]).await?;

    assert!(ids_with_tag(&database, &existing).await?.is_empty());
    let catalog_after_clear = database.list_tags().await?;
    assert_eq!(catalog_after_clear, catalog_before_clear);

    let unassigned = TagName::new("Must not be catalogued")?;
    let failed_assignment = database.set_food_tags(FoodId(-1), &[unassigned]).await;
    assert!(matches!(failed_assignment, Err(sqlx::Error::RowNotFound)));
    assert_eq!(database.list_tags().await?, catalog_after_clear);

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn failed_tag_replacement_rolls_back_assignments_and_catalog_rows()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let food = make_food(
        Source::Borrowed("community"),
        "food-4",
        "Community",
        "Stable food",
        None,
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    let food_id = database.upsert_food(&food).await?;
    let existing = TagName::new("Existing")?;
    database
        .set_food_tags(food_id, std::slice::from_ref(&existing))
        .await?;
    let original_catalog = database.list_tags().await?;

    sqlx::query(
        "CREATE TRIGGER reject_blocked_tag
         BEFORE INSERT ON crumbs_tags
         WHEN NEW.name = 'blocked'
         BEGIN
             SELECT RAISE(ABORT, 'blocked tag');
         END",
    )
    .execute(&database.pool)
    .await?;

    let first_new_tag = TagName::new("new before failure")?;
    let blocked_tag = TagName::new("blocked")?;
    let failed_replacement = database
        .set_food_tags(food_id, &[first_new_tag.clone(), blocked_tag.clone()])
        .await;

    assert!(failed_replacement.is_err());
    assert_eq!(ids_with_tag(&database, &existing).await?, vec![food_id]);
    assert!(ids_with_tag(&database, &first_new_tag).await?.is_empty());
    assert!(ids_with_tag(&database, &blocked_tag).await?.is_empty());
    assert_eq!(database.list_tags().await?, original_catalog);

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

fn make_food(
    source: Source,
    source_food_id: &str,
    food_type: &'static str,
    description: &str,
    category: Option<&str>,
    source_data: &str,
) -> Result<Food, Box<dyn StdError>> {
    Ok(Food {
        source,
        source_food_id: source_food_id.to_owned(),
        food_type: Cow::Borrowed(food_type),
        description: description.to_owned(),
        category: category.map(str::to_owned),
        parsed_data: serde_json::from_str::<ParsedFoodData>(source_data)?,
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

async fn ids_with_tag(database: &Database, tag: &TagName) -> Result<Vec<FoodId>, sqlx::Error> {
    type RowId = i64;

    Ok(sqlx::query_scalar::<_, RowId>(
        "SELECT ft.food_id FROM crumbs_food_tags ft
         JOIN crumbs_tags t ON t.id = ft.tag_id
         WHERE t.name_key = ? ORDER BY ft.food_id",
    )
    .bind(&tag.key)
    .fetch_all(&database.pool)
    .await?
    .into_iter()
    .map(FoodId)
    .collect())
}

fn write_input(directory: &Path, name: &str, contents: &str) -> Result<PathBuf, io::Error> {
    let path = directory.join(name);
    fs::write(&path, contents)?;
    Ok(path)
}
