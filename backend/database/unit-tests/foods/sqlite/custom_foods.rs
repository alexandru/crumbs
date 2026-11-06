use std::error::Error as StdError;
use std::io;

use crate::foods::tests::{ids_with_tag, make_food, open_database};
use crate::foods::*;

#[tokio::test]
async fn custom_food_crud_replaces_fields_preserves_identity_and_cleans_search()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let initial = CustomFoodInput {
        description: CustomFoodDescription::new("  House lentils  ")?,
        category: Some("Beans".to_owned()),
        notes: Some("familyonlysecret".to_owned()),
        parsed_data: crate::nutrition_model::FoodData::default(),
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
        .search_foods(&private_note_query, FoodLimit::default())
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
        Some(crate::nutrition_model::NutritionBasis::Per100Grams)
    );

    let query = SearchQuery::new("tags:favorite")?;
    let results = database.search_foods(&query, FoodLimit::default()).await?;
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

    let listed = database.list_custom_foods(FoodLimit::default()).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert!(database.delete_custom_food(id).await?);
    assert!(database.custom_food(id).await?.is_none());
    assert!(!database.delete_custom_food(id).await?);
    assert!(ids_with_tag(&database, &favorite).await?.is_empty());
    let remaining = database.search_foods(&query, FoodLimit::default()).await?;
    assert!(remaining.custom.is_empty());

    database.close().await;
    Ok(())
}
