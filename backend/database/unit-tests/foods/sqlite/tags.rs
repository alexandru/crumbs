mod assignments {
    use std::error::Error as StdError;

    use crate::foods::tests::{ids_with_tag, make_food, open_database};
    use crate::foods::*;

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
    async fn tag_names_fold_unicode_case_without_removing_accents() -> Result<(), Box<dyn StdError>>
    {
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
        assert!(matches!(
            failed_assignment
                .as_ref()
                .err()
                .and_then(|error| error.source())
                .and_then(|source| source.downcast_ref::<sqlx::Error>()),
            Some(sqlx::Error::RowNotFound)
        ));
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
}

mod timestamps {
    use std::error::Error as StdError;
    use std::io;

    use crate::foods::sqlite::tests::{
        Timestamps, assignment_timestamps, at, has_assignment, make_food, set_clock, tag_timestamps,
    };
    use crate::foods::tests::open_database;
    use crate::foods::*;

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
    async fn catalog_conflicts_keep_first_spelling_and_timestamps() -> Result<(), Box<dyn StdError>>
    {
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
}
