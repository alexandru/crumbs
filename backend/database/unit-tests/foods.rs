use std::error::Error as StdError;
use std::io;

use super::*;
use crate::{Database, DatabaseConfig};

#[tokio::test]
async fn upsert_foods_are_scoped_by_provider_and_keep_opaque_ids() -> Result<(), Box<dyn StdError>>
{
    let (_directory, database) = open_database().await?;
    let source = Source::Owned("regional catalog/β".to_owned());
    let source_food_id = "provider-record:food/ß".to_owned();
    let mut initial = make_food(
        source.clone(),
        &source_food_id,
        Dataset::Common,
        "Original food",
        Some("Original category"),
        r#"{"nutrition":null,"serving_sizes":[]}"#,
    )?;
    initial.notes = Some("source supplied note".to_owned());

    let first_id = database.upsert_food(&initial).await?;
    let initial_summary = find_food(&database, &source, &source_food_id).await?;
    assert_eq!(
        initial_summary.notes.as_deref(),
        Some("source supplied note")
    );
    let updated = make_food(
        source.clone(),
        &source_food_id,
        Dataset::Common,
        "Updated food",
        Some("Updated category"),
        r#"{"nutrition":{"per":"100 g"},"serving_sizes":[]}"#,
    )?;
    let updated_id = database.upsert_food(&updated).await?;

    assert_eq!(updated_id, first_id);
    let summary = find_food(&database, &source, &source_food_id).await?;
    assert_eq!(summary.id, first_id);
    assert_eq!(summary.source_food_id, source_food_id);
    assert_eq!(summary.description, "Updated food");
    assert_eq!(summary.category.as_deref(), Some("Updated category"));
    assert!(summary.notes.is_none());
    let parsed_data_json: String = sqlx::query_scalar(
        "SELECT parsed_data FROM crumbs_foods WHERE source = ? AND source_food_id = ?",
    )
    .bind(source.as_ref())
    .bind(&source_food_id)
    .fetch_one(&database.pool)
    .await?;
    let parsed_data: crate::nutrition::FoodData = serde_json::from_str(&parsed_data_json)?;
    assert_eq!(
        parsed_data.nutrition.map(|profile| profile.per),
        Some(crate::nutrition::NutritionBasis::Per100Grams)
    );

    let other_source = Source::Owned("another catalog".to_owned());
    let same_source_id = make_food(
        other_source.clone(),
        &source_food_id,
        Dataset::Branded,
        "Other provider food",
        None,
        "{}",
    )?;
    let other_id = database.upsert_food(&same_source_id).await?;
    assert_ne!(other_id, first_id);
    let other_summary = find_food(&database, &other_source, &source_food_id).await?;
    assert_eq!(other_summary.id, other_id);

    let leading_zero_id = "0000123456789";
    let leading_zero_food = make_food(
        source.clone(),
        leading_zero_id,
        Dataset::Common,
        "Leading zero identifier",
        None,
        "{}",
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
        Dataset::Custom,
        "Oat drink",
        None,
        "{}",
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
        Dataset::Custom,
        "Romanian food",
        None,
        "{}",
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
async fn clearing_assignments_keeps_catalog_and_unknown_food_does_not_change_it()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let food = make_food(
        Source::Borrowed("community"),
        "food-3",
        Dataset::Custom,
        "Tagged food",
        None,
        "{}",
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
        Dataset::Custom,
        "Stable food",
        None,
        "{}",
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

#[test]
fn dataset_and_custom_food_input_enforce_their_wire_contracts() -> Result<(), Box<dyn StdError>> {
    assert_eq!(
        serde_json::from_str::<Dataset>("\"common\"")?,
        Dataset::Common
    );
    assert_eq!(
        serde_json::from_str::<Dataset>("\"branded\"")?,
        Dataset::Branded
    );
    assert_eq!(
        serde_json::from_str::<Dataset>("\"custom\"")?,
        Dataset::Custom
    );
    assert!("usda_foundation".parse::<Dataset>().is_err());

    let input: CustomFoodInput = serde_json::from_str(r#"{"description":"  House sauce  "}"#)?;
    assert_eq!(input.description.as_str(), "  House sauce  ");
    assert!(input.category.is_none());
    assert!(input.notes.is_none());
    assert_eq!(input.parsed_data, crate::nutrition::FoodData::default());
    assert!(CustomFoodDescription::new(" \t\n").is_err());
    let blank_description_json = serde_json::to_string(" \t\n")?;
    assert!(serde_json::from_str::<CustomFoodDescription>(&blank_description_json).is_err());
    for invalid_description in ["\0", "\0House sauce", "House\0 sauce"] {
        assert!(CustomFoodDescription::new(invalid_description).is_err());
        let invalid_description_json = serde_json::to_string(invalid_description)?;
        assert!(serde_json::from_str::<CustomFoodDescription>(&invalid_description_json).is_err());
    }

    for forbidden in ["id", "source", "source_food_id", "dataset"] {
        let payload = format!(r#"{{"description":"Valid food","{forbidden}":"custom"}}"#);
        assert!(serde_json::from_str::<CustomFoodInput>(&payload).is_err());
    }
    Ok(())
}

#[tokio::test]
async fn custom_food_crud_replaces_fields_preserves_identity_and_cleans_search()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let initial = CustomFoodInput {
        description: CustomFoodDescription::new("  House lentils  ")?,
        category: Some("Beans".to_owned()),
        notes: Some("familyonlysecret".to_owned()),
        parsed_data: crate::nutrition::FoodData::default(),
    };
    let created = database.create_custom_food(&initial).await?;
    let id = created.id;
    let source_food_id = created.food.source_food_id.clone();
    assert_eq!(created.food.source.as_ref(), "custom");
    assert_eq!(created.food.dataset, Dataset::Custom);
    assert_eq!(created.food.description, "  House lentils  ");
    assert_eq!(created.food.notes.as_deref(), Some("familyonlysecret"));
    assert_eq!(source_food_id.len(), 32);
    assert!(source_food_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let fetched = database
        .custom_food(id)
        .await?
        .ok_or_else(|| io::Error::other("created custom food was not found by local ID"))?;
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.food.notes.as_deref(), Some("familyonlysecret"));

    let private_note_query = SearchQuery::new("familyonlysecret")?;
    let note_results = database
        .search_foods(&private_note_query, SearchLimit::default())
        .await?;
    assert!(note_results.common.is_empty());
    assert!(note_results.branded.is_empty());
    assert!(note_results.custom.is_empty());

    let favorite = TagName::new("favorite")?;
    database
        .set_food_tags(id, std::slice::from_ref(&favorite))
        .await?;

    let summary = database
        .food_by_source(&Source::Borrowed("custom"), &source_food_id)
        .await?
        .ok_or_else(|| io::Error::other("created custom food was not found by source identity"))?;
    assert_eq!(summary.notes.as_deref(), Some("familyonlysecret"));

    let replacement = CustomFoodInput {
        description: CustomFoodDescription::new("Updated lentils")?,
        category: None,
        notes: None,
        parsed_data: serde_json::from_str(r#"{"nutrition":{"per":"100 g"},"serving_sizes":[]}"#)?,
    };
    let updated = database
        .update_custom_food(id, &replacement)
        .await?
        .ok_or_else(|| io::Error::other("custom food update found no food"))?;
    assert_eq!(updated.id, id);
    assert_eq!(updated.food.source_food_id, source_food_id);
    assert_eq!(updated.food.description, "Updated lentils");
    assert!(updated.food.category.is_none());
    assert!(updated.food.notes.is_none());
    assert_eq!(
        updated
            .food
            .parsed_data
            .nutrition
            .as_ref()
            .map(|profile| profile.per),
        Some(crate::nutrition::NutritionBasis::Per100Grams)
    );

    let query = SearchQuery::new("tags:favorite")?;
    let results = database
        .search_foods(&query, SearchLimit::default())
        .await?;
    assert!(results.common.is_empty());
    assert!(results.branded.is_empty());
    assert_eq!(results.custom.len(), 1);
    assert_eq!(results.custom[0].id, id);

    let imported = make_food(
        Source::Borrowed("usda"),
        "fdc-1",
        Dataset::Common,
        "Imported food",
        None,
        "{}",
    )?;
    let imported_id = database.upsert_food(&imported).await?;
    let custom_source_common = make_food(
        Source::Borrowed("custom"),
        "non-custom-dataset",
        Dataset::Common,
        "Not a custom food dataset",
        None,
        "{}",
    )?;
    let custom_source_common_id = database.upsert_food(&custom_source_common).await?;
    for non_custom_id in [imported_id, custom_source_common_id, FoodId(-1)] {
        assert!(database.custom_food(non_custom_id).await?.is_none());
        assert!(
            database
                .update_custom_food(non_custom_id, &replacement)
                .await?
                .is_none()
        );
        assert!(!database.delete_custom_food(non_custom_id).await?);
    }

    let listed = database.list_custom_foods(SearchLimit::default()).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert!(database.delete_custom_food(id).await?);
    assert!(database.custom_food(id).await?.is_none());
    assert!(!database.delete_custom_food(id).await?);
    assert!(ids_with_tag(&database, &favorite).await?.is_empty());
    let remaining = database
        .search_foods(&query, SearchLimit::default())
        .await?;
    assert!(remaining.custom.is_empty());

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
