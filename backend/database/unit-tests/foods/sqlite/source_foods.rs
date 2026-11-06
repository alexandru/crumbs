mod records {
    use std::error::Error as StdError;

    use crate::foods::tests::{find_food, make_food, open_database};
    use crate::foods::*;

    #[tokio::test]
    async fn upsert_foods_are_scoped_by_provider_and_keep_opaque_ids()
    -> Result<(), Box<dyn StdError>> {
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
        let parsed_data: crate::nutrition_model::FoodData =
            serde_json::from_str(&parsed_data_json)?;
        assert_eq!(
            parsed_data.nutrition.map(|profile| profile.per),
            Some(crate::nutrition_model::NutritionBasis::Per100Grams)
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
}

mod timestamps {
    use std::error::Error as StdError;
    use std::io;

    use crate::foods::sqlite::tests::Timestamps;
    use crate::foods::sqlite::tests::{
        at, food_timestamps, make_food, set_clock, source_timestamps,
    };
    use crate::foods::tests::open_database;
    use crate::foods::*;

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
}
