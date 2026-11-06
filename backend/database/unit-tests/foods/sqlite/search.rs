mod queries {
    use std::error::Error as StdError;
    use std::io;

    use crate::Database;
    use crate::foods::sqlite::tests::make_food;
    use crate::foods::tests::open_database;
    use crate::foods::*;

    #[tokio::test]
    async fn searches_indexed_fields_and_returns_full_nutrition() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        let mut food = make_food("usda", "1", "Roșii coapte")?;
        food.category = Some("Produce".to_owned());
        let mut nutrition = crate::nutrition_model::NutritionProfile::empty(
            crate::nutrition_model::NutritionBasis::Per100Grams,
        );
        nutrition.energy = Some("18 kcal".parse()?);
        nutrition.protein = Some("0.9 g".parse()?);
        food.parsed_data = crate::nutrition_model::FoodData {
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
            let results = search_database(&database, expression, FoodLimit::default()).await?;
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
    async fn search_applies_the_limit_independently_to_each_dataset()
    -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        for index in 0..21 {
            database
                .upsert_food(&make_food(
                    "common-provider",
                    &index.to_string(),
                    &format!("Chicken common {index}"),
                )?)
                .await?;
            let mut branded = make_food(
                "branded-provider",
                &index.to_string(),
                &format!("Chicken branded {index}"),
            )?;
            branded.dataset = Dataset::Branded;
            database.upsert_food(&branded).await?;
            database
                .create_custom_food(&CustomFoodInput {
                    description: CustomFoodDescription::new(format!("Chicken custom {index}"))?,
                    category: None,
                    notes: None,
                    parsed_data: crate::nutrition_model::FoodData::default(),
                })
                .await?;
        }

        let results = database
            .search_foods(&SearchQuery::new("chicken")?, FoodLimit::from(20))
            .await?;
        assert_eq!(results.common.len(), 20);
        assert_eq!(results.branded.len(), 20);
        assert_eq!(results.custom.len(), 20);
        Ok(())
    }

    #[tokio::test]
    async fn search_returns_empty_vectors_for_datasets_without_matches()
    -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        database
            .upsert_food(&make_food("common-provider", "common-1", "Chicken soup")?)
            .await?;

        let results = database
            .search_foods(&SearchQuery::new("chicken")?, FoodLimit::default())
            .await?;
        assert_eq!(results.common.len(), 1);
        assert!(results.branded.is_empty());
        assert!(results.custom.is_empty());
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
    async fn search_returns_empty_for_unmatched_valid_expression() -> Result<(), Box<dyn StdError>>
    {
        let (_directory, database) = open_database().await?;
        database
            .upsert_food(&make_food("usda", "1", "Plain rice")?)
            .await?;
        assert!(
            search_database(&database, "unicorn", FoodLimit::default())
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
                .search_foods(&SearchQuery::new(expression)?, FoodLimit::default())
                .await
                .err()
                .ok_or_else(|| io::Error::other("expected invalid FTS5 expression error"))?;
            assert!(matches!(error, SearchError::InvalidQuery(_)), "{error}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn missing_search_column_is_reported_as_storage_failure() -> Result<(), Box<dyn StdError>>
    {
        let (_directory, database) = open_database().await?;
        database
            .upsert_food(&make_food("usda", "1", "Milk")?)
            .await?;
        sqlx::query("ALTER TABLE crumbs_foods RENAME COLUMN parsed_data TO parsed_data_broken")
            .execute(&database.pool)
            .await?;

        let error = database
            .search_foods(&SearchQuery::new("milk")?, FoodLimit::default())
            .await
            .err()
            .ok_or_else(|| io::Error::other("expected a storage failure"))?;
        assert!(matches!(error, SearchError::Storage(_)), "{error}");
        Ok(())
    }

    #[tokio::test]
    async fn missing_search_index_is_reported_as_storage_failure() -> Result<(), Box<dyn StdError>>
    {
        let (_directory, database) = open_database().await?;
        sqlx::query("DROP TABLE crumbs_foods_fts")
            .execute(&database.pool)
            .await?;

        let error = database
            .search_foods(&SearchQuery::new("milk")?, FoodLimit::default())
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
            .search_foods(&SearchQuery::new(expression)?, FoodLimit::default())
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
            search_database(&database, "chicken", FoodLimit::from(1))
                .await?
                .len(),
            20
        );
        Ok(())
    }

    async fn search_database(
        database: &Database,
        expression: &str,
        limit: FoodLimit,
    ) -> Result<Vec<StoredFood>, Box<dyn StdError>> {
        Ok(database
            .search_foods(&SearchQuery::new(expression)?, limit)
            .await?
            .common)
    }

    async fn search_food_ids(
        database: &Database,
        expression: &str,
    ) -> Result<Vec<FoodId>, Box<dyn StdError>> {
        Ok(search_database(database, expression, FoodLimit::default())
            .await?
            .into_iter()
            .map(|result| result.id)
            .collect())
    }
}

mod fts {
    use std::error::Error as StdError;
    use std::fs;

    use sqlx::migrate::Migrator;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
    use sqlx::{SqliteConnection, query, query_as, query_scalar};

    use crate::foods::tests::open_database;
    use crate::foods::{
        FoodCategory, FoodDescription, FoodLimit, FoodNotes, FoodSearch, SearchQuery, TagName,
        TagStore,
    };
    use crate::{Database, DatabaseConfig};

    const MIGRATION_0001: &str = include_str!("../../../migrations/0001_create_crumbs_sources.sql");
    const MIGRATION_0002: &str = include_str!("../../../migrations/0002_create_crumbs_foods.sql");
    type FoodId = i64;
    type TagId = i64;
    type FoodCount = i64;
    type MigrationCount = i64;
    type IndexedTags = String;
    type DatasetText = String;
    type FtsTerm = String;
    type Source<'a> = &'a str;
    type SourceFoodId<'a> = &'a str;
    type RawFoodDescription<'a> = &'a str;
    type RawFoodCategory<'a> = &'a str;
    type RawTagName<'a> = &'a str;
    type TagKey<'a> = &'a str;
    type FtsQuery<'a> = &'a str;

    #[tokio::test]
    async fn schema_uses_external_content_unicode_tokenizer_and_triggers()
    -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;

        let sql: String =
            query_scalar("SELECT sql FROM sqlite_master WHERE name = 'crumbs_foods_fts'")
                .fetch_one(&database.pool)
                .await?;
        assert!(
            sql.contains("content = 'crumbs_foods_search_content'"),
            "{sql}"
        );
        assert!(sql.contains("content_rowid = 'id'"), "{sql}");
        assert!(sql.contains("remove_diacritics 2"), "{sql}");

        assert_eq!(fts_row_count(&database).await?, 0);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn english_matching_is_case_insensitive_without_stemming() -> Result<(), Box<dyn StdError>>
    {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "usda",
            "en-1",
            "Crunchy Peanut Butter",
            Some("Pantry Staples"),
            "Smooth, Salty",
        )
        .await?;
        insert_food(
            &database,
            2,
            "usda",
            "en-2",
            "Cheddar Cheese",
            Some("Dairy"),
            "Sharp",
        )
        .await?;

        for query in ["crunchy", "CRUNCHY", "CrUnChY", "peanut", "BUTTER"] {
            assert_matches(&database, query, &[1]).await?;
        }
        for query in ["pantry", "PANTRY", "smooth", "SALTY"] {
            assert_matches(&database, query, &[1]).await?;
        }
        assert_matches(&database, "cheddar", &[2]).await?;
        assert_matches(&database, "category:dairy", &[2]).await?;
        assert_matches(&database, "tags:sharp", &[2]).await?;
        assert_matches(&database, "crunch", &[]).await?;
        assert_matches(&database, "heddar", &[]).await?;
        assert_matches(&database, "cheeses", &[]).await?;
        assert_eq!(description(&database, 1).await?, "Crunchy Peanut Butter");
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn source_and_dataset_are_searchable_fields() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(&database, 1, "usda", "fdc-1", "Chicken breast", None, "").await?;
        insert_food(&database, 2, "other", "other-1", "Chicken breast", None, "").await?;
        insert_food(&database, 3, "usda", "fdc-2", "Chicken thigh", None, "").await?;
        insert_food(&database, 4, "usda", "fdc-3", "Survey pancakes", None, "").await?;
        query("UPDATE crumbs_foods SET dataset = 'common' WHERE id = 1")
            .execute(&database.pool)
            .await?;
        query("UPDATE crumbs_foods SET dataset = 'branded' WHERE id = 2")
            .execute(&database.pool)
            .await?;
        query("UPDATE crumbs_foods SET dataset = 'common' WHERE id = 3")
            .execute(&database.pool)
            .await?;
        query("UPDATE crumbs_foods SET dataset = 'common' WHERE id = 4")
            .execute(&database.pool)
            .await?;

        assert_search_matches(&database, "dataset:common", &[1, 3, 4]).await?;
        assert_search_matches(&database, "source:USDA", &[1, 3, 4]).await?;
        assert_search_matches(&database, "chicken source:usda", &[1, 3]).await?;
        assert_search_matches(&database, "chicken source:usda AND dataset:common", &[1, 3]).await?;
        assert_search_matches(&database, "dataset:branded", &[2]).await?;
        query("INSERT INTO crumbs_sources (id, created_at) VALUES ('community', 0)")
            .execute(&database.pool)
            .await?;
        query("UPDATE crumbs_foods SET source = 'community' WHERE id = 1")
            .execute(&database.pool)
            .await?;
        assert_search_matches(&database, "source:usda", &[3, 4]).await?;
        assert_search_matches(&database, "source:community", &[1]).await?;
        query("UPDATE crumbs_foods SET dataset = 'custom' WHERE id = 1")
            .execute(&database.pool)
            .await?;
        assert_search_matches(&database, "dataset:common", &[3, 4]).await?;
        assert_search_matches(&database, "dataset:custom", &[1]).await?;
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn romanian_diacritics_match_english_case_and_all_variants()
    -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        let rows = [
            (1, "Roșii coapte", Some("Legume"), "roșu"),
            (2, "Roşii coapte", None, "roşu"),
            (3, "Țuică fiartă", Some("Băuturi"), ""),
            (4, "Ţuică fiartă", None, ""),
            (5, "Măr dulce", Some("Fructe"), "proaspăt"),
            (6, "Mar dulce", Some("Fructe"), "proaspat"),
            (7, "Câmpie verde", None, ""),
            (8, "Campie verde", None, ""),
            (9, "Cîntec vechi", Some("Muzică"), ""),
            (10, "Cintec vechi", Some("Muzica"), ""),
            (11, "Cântec nou", Some("Muzică"), ""),
            (12, "Ștefan cel Mare", None, ""),
            (13, "Ştefan cel Mare", None, ""),
            (14, "Mere proaspete", Some("Fructe"), ""),
            (15, "Proaspăt", None, ""),
        ];
        for (id, description, category, tags) in rows {
            insert_food(
                &database,
                id,
                "usda",
                &format!("ro-{id}"),
                description,
                category,
                tags,
            )
            .await?;
        }

        let queries = [
            ("rosii", &[1, 2][..]),
            ("tuica", &[3, 4][..]),
            ("campie", &[7, 8][..]),
            ("cintec", &[9, 10][..]),
            ("mar", &[5, 6][..]),
            ("stefan", &[12, 13][..]),
            ("proaspat", &[5, 6, 15][..]),
            ("roșii", &[1, 2][..]),
            ("roşii", &[1, 2][..]),
            ("țuică", &[3, 4][..]),
            ("ţuică", &[3, 4][..]),
            ("câmpie", &[7, 8][..]),
            ("cîntec", &[9, 10][..]),
            ("măr", &[5, 6][..]),
            ("ștefan", &[12, 13][..]),
            ("ştefan", &[12, 13][..]),
            ("ROȘII", &[1, 2][..]),
            ("ȘTEFAN", &[12, 13][..]),
            ("ŞTEFAN", &[12, 13][..]),
            ("cantec", &[11][..]),
            ("cântec", &[11][..]),
            ("cîntec", &[9, 10][..]),
            ("mere", &[14][..]),
            ("proaspete", &[14][..]),
            ("rosi", &[][..]),
            ("apple", &[][..]),
        ];
        for (query, expected) in queries {
            assert_matches(&database, query, expected).await?;
        }
        assert_matches(&database, "category:legume", &[1]).await?;
        assert_matches(&database, "category:bauturi", &[3]).await?;
        assert_matches(&database, "category:fructe", &[5, 6, 14]).await?;
        assert_matches(&database, "category:muzica", &[9, 10, 11]).await?;
        assert_matches(&database, "tags:rosu", &[1, 2]).await?;
        assert_matches(&database, "description:proaspat", &[15]).await?;
        assert_matches(&database, "category:rosu", &[]).await?;
        assert_eq!(description(&database, 1).await?, "Roșii coapte");
        assert_eq!(description(&database, 2).await?, "Roşii coapte");

        let terms = vocab_terms(&database).await?;
        for term in [
            "rosii", "tuica", "campie", "cintec", "mar", "stefan", "proaspat",
        ] {
            assert!(
                terms.iter().any(|actual| actual == term),
                "missing {term}: {terms:?}"
            );
        }
        assert!(!terms.iter().any(|actual| actual == "măr"));
        assert!(!terms.iter().any(|actual| actual == "ștefan"));
        assert_eq!(fts_row_count(&database).await?, 15);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn decomposed_romanian_forms_match_composed_and_plain_forms()
    -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        let families = [
            (
                "s + U+0326",
                "\u{0326}",
                "\u{0219}urub",
                "s\u{0326}urub",
                "surub",
            ),
            (
                "t + U+0326",
                "\u{0326}",
                "\u{021b}esut",
                "t\u{0326}esut",
                "tesut",
            ),
            ("a + U+0306", "\u{0306}", "m\u{0103}r", "ma\u{0306}r", "mar"),
            (
                "a + U+0302",
                "\u{0302}",
                "c\u{00e2}mp",
                "ca\u{0302}mp",
                "camp",
            ),
            (
                "i + U+0302",
                "\u{0302}",
                "c\u{00ee}ntec",
                "ci\u{0302}ntec",
                "cintec",
            ),
        ];

        let mut next_id = 1;
        for (_, mark, composed, decomposed, plain) in families {
            assert!(decomposed.contains(mark));
            assert!(!composed.contains(mark));
            let ids = [next_id, next_id + 1, next_id + 2];
            insert_food(
                &database,
                ids[0],
                "usda",
                &format!("d-{next_id}"),
                composed,
                None,
                "",
            )
            .await?;
            insert_food(
                &database,
                ids[1],
                "usda",
                &format!("d-{}", next_id + 1),
                decomposed,
                None,
                "",
            )
            .await?;
            insert_food(
                &database,
                ids[2],
                "usda",
                &format!("d-{}", next_id + 2),
                plain,
                None,
                "",
            )
            .await?;
            for query in [composed, decomposed, plain] {
                assert_matches(&database, query, &ids).await?;
            }
            assert_eq!(description(&database, ids[1]).await?, decomposed);
            next_id += 3;
        }

        let terms = vocab_terms(&database).await?;
        for (_, _, _, _, plain) in families {
            assert!(
                terms.iter().filter(|term| term.as_str() == plain).count() == 1,
                "expected one folded token for {plain}: {terms:?}"
            );
        }
        assert_eq!(fts_row_count(&database).await?, 15);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn updates_clear_null_categories_tags_and_keep_unchanged_rows_stable()
    -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "usda",
            "mutable",
            "Mutable food",
            Some("Oldcat"),
            "oldtag",
        )
        .await?;
        assert_matches(&database, "oldcat", &[1]).await?;
        assert_matches(&database, "oldtag", &[1]).await?;

        query(
            "UPDATE crumbs_foods
         SET description = 'Changed food', category = 'Newcat'
         WHERE id = 1",
        )
        .execute(&database.pool)
        .await?;
        let new_tag = TagName::new("newtag")?;
        database
            .set_food_tags(crate::foods::FoodId(1), &[new_tag])
            .await?;
        assert_matches(&database, "mutable", &[]).await?;
        assert_matches(&database, "oldcat", &[]).await?;
        assert_matches(&database, "oldtag", &[]).await?;
        assert_matches(&database, "changed", &[1]).await?;
        assert_matches(&database, "newcat", &[1]).await?;
        assert_matches(&database, "newtag", &[1]).await?;

        query("UPDATE crumbs_foods SET category = NULL WHERE id = 1")
            .execute(&database.pool)
            .await?;
        database.set_food_tags(crate::foods::FoodId(1), &[]).await?;
        assert_matches(&database, "newcat", &[]).await?;
        assert_matches(&database, "newtag", &[]).await?;
        assert_matches(&database, "changed", &[1]).await?;
        assert_eq!(category(&database, 1).await?, None);
        assert_eq!(tags(&database, 1).await?, "");

        query("UPDATE crumbs_foods SET description = 'Changed food', category = NULL WHERE id = 1")
            .execute(&database.pool)
            .await?;
        assert_matches(&database, "changed", &[1]).await?;
        assert_eq!(fts_row_count(&database).await?, 1);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn cross_source_foods_are_independently_indexed() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "usda",
            "shared",
            "Shared Orchard Apple",
            Some("Fruit"),
            "crisp",
        )
        .await?;
        insert_food(
            &database,
            2,
            "other",
            "shared",
            "Shared Orchard Apple",
            Some("Other Fruit"),
            "othertag",
        )
        .await?;

        assert_matches(&database, "shared", &[1, 2]).await?;
        assert_matches(&database, "crisp", &[1]).await?;
        assert_matches(&database, "othertag", &[2]).await?;

        query(
            "UPDATE crumbs_foods
         SET description = 'Changed Other', category = 'Other Changed'
         WHERE source = 'other' AND id = 2",
        )
        .execute(&database.pool)
        .await?;
        let changed_tag = TagName::new("changedtag")?;
        database
            .set_food_tags(crate::foods::FoodId(2), &[changed_tag])
            .await?;
        assert_matches(&database, "shared", &[1]).await?;
        assert_matches(&database, "othertag", &[]).await?;
        assert_matches(&database, "changed", &[2]).await?;
        assert_matches(&database, "tags:changedtag", &[2]).await?;
        assert_eq!(description(&database, 1).await?, "Shared Orchard Apple");
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn public_tagging_and_replacement_update_the_index() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(&database, 1, "community", "tagged", "Tagged food", None, "").await?;

        let vegan = TagName::new("Vegan")?;
        let lowercase_vegan = TagName::new("vegan")?;
        database
            .set_food_tags(crate::foods::FoodId(1), &[vegan, lowercase_vegan])
            .await?;
        assert_matches(&database, "tags:vegan", &[1]).await?;
        assert_fts_integrity(&database).await?;

        let regional = TagName::new("regional")?;
        database
            .set_food_tags(crate::foods::FoodId(1), &[regional])
            .await?;
        assert_matches(&database, "tags:vegan", &[]).await?;
        assert_matches(&database, "tags:regional", &[1]).await?;
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn renaming_a_shared_tag_reindexes_each_assigned_food() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "community",
            "shared-one",
            "First shared food",
            None,
            "",
        )
        .await?;
        insert_food(
            &database,
            2,
            "community",
            "shared-two",
            "Second shared food",
            None,
            "",
        )
        .await?;
        let shared = TagName::new("sharedtag")?;
        database
            .set_food_tags(crate::foods::FoodId(1), std::slice::from_ref(&shared))
            .await?;
        database
            .set_food_tags(crate::foods::FoodId(2), &[shared])
            .await?;
        assert_matches(&database, "tags:sharedtag", &[1, 2]).await?;

        let id = tag_id(&database, "sharedtag").await?;
        query("UPDATE crumbs_tags SET name = ?, name_key = ? WHERE id = ?")
            .bind("renamedtag")
            .bind("renamedtag")
            .bind(id)
            .execute(&database.pool)
            .await?;

        assert_matches(&database, "tags:sharedtag", &[]).await?;
        assert_matches(&database, "tags:renamedtag", &[1, 2]).await?;
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn deleting_a_tag_removes_its_terms_and_keeps_other_tags() -> Result<(), Box<dyn StdError>>
    {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "community",
            "tag-delete",
            "Tagged food",
            None,
            "",
        )
        .await?;
        let obsolete = TagName::new("obsolete")?;
        let retained = TagName::new("retained")?;
        database
            .set_food_tags(crate::foods::FoodId(1), &[obsolete, retained])
            .await?;
        assert_matches(&database, "tags:obsolete", &[1]).await?;
        assert_matches(&database, "tags:retained", &[1]).await?;

        let id = tag_id(&database, "obsolete").await?;
        query("DELETE FROM crumbs_tags WHERE id = ?")
            .bind(id)
            .execute(&database.pool)
            .await?;

        assert_matches(&database, "tags:obsolete", &[]).await?;
        assert_matches(&database, "tags:retained", &[1]).await?;
        assert_eq!(fts_row_count(&database).await?, 1);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn moving_a_tag_assignment_reindexes_both_foods() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "community",
            "move-one",
            "First food",
            None,
            "",
        )
        .await?;
        insert_food(
            &database,
            2,
            "community",
            "move-two",
            "Second food",
            None,
            "",
        )
        .await?;
        let moving = TagName::new("move")?;
        let first_only = TagName::new("firstonly")?;
        let second_only = TagName::new("secondonly")?;
        database
            .set_food_tags(
                crate::foods::FoodId(1),
                &[moving.clone(), first_only.clone()],
            )
            .await?;
        database
            .set_food_tags(crate::foods::FoodId(2), std::slice::from_ref(&second_only))
            .await?;
        assert_matches(&database, "tags:move", &[1]).await?;

        let id = tag_id(&database, "move").await?;
        query(
            "UPDATE crumbs_food_tags SET food_id = 2
         WHERE food_id = 1 AND tag_id = ?",
        )
        .bind(id)
        .execute(&database.pool)
        .await?;

        assert_matches(&database, "tags:move", &[2]).await?;
        assert_matches(&database, "tags:firstonly", &[1]).await?;
        assert_matches(&database, "tags:secondonly", &[2]).await?;
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn direct_rowid_update_and_delete_keep_index_consistent() -> Result<(), Box<dyn StdError>>
    {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "usda",
            "keep",
            "Keep this row",
            Some("Keep"),
            "keep",
        )
        .await?;
        insert_food(
            &database,
            2,
            "usda",
            "move",
            "Direct alpha",
            Some("Cat"),
            "tag",
        )
        .await?;

        query("UPDATE crumbs_foods SET id = 20 WHERE id = 2")
            .execute(&database.pool)
            .await?;
        assert_matches(&database, "alpha", &[20]).await?;
        assert_matches(&database, "keep", &[1]).await?;
        assert_matches(&database, "tags:tag", &[20]).await?;
        assert_eq!(indexed_rowid(&database, "alpha").await?, Some(20));
        assert_eq!(indexed_rowid_count(&database, 2).await?, 0);

        let tag = tag_id(&database, "tag").await?;
        query("UPDATE crumbs_tags SET id = 100 WHERE id = ?")
            .bind(tag)
            .execute(&database.pool)
            .await?;
        assert_matches(&database, "tags:tag", &[20]).await?;

        query(
            "UPDATE crumbs_foods
         SET id = 30, description = 'Direct beta', category = NULL
         WHERE id = 20",
        )
        .execute(&database.pool)
        .await?;
        let tag2 = TagName::new("tag2")?;
        database
            .set_food_tags(crate::foods::FoodId(30), &[tag2])
            .await?;
        assert_matches(&database, "alpha", &[]).await?;
        assert_matches(&database, "beta", &[30]).await?;
        assert_matches(&database, "tags:tag", &[]).await?;
        assert_matches(&database, "tag2", &[30]).await?;
        assert_eq!(indexed_rowid_count(&database, 20).await?, 0);

        query("DELETE FROM crumbs_foods WHERE id = 30")
            .execute(&database.pool)
            .await?;
        assert_matches(&database, "beta", &[]).await?;
        assert_matches(&database, "tag2", &[]).await?;
        assert_matches(&database, "keep", &[1]).await?;
        assert_eq!(fts_row_count(&database).await?, 1);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn transaction_rollback_restores_food_and_index() -> Result<(), Box<dyn StdError>> {
        let (_directory, database) = open_database().await?;
        insert_food(
            &database,
            1,
            "usda",
            "baseline",
            "Committed baseline",
            Some("Stable"),
            "keep",
        )
        .await?;
        insert_food(
            &database,
            2,
            "usda",
            "other-tagged",
            "Other committed food",
            Some("Other"),
            "retained",
        )
        .await?;

        let mut update = database.pool.begin().await?;
        query(
            "UPDATE crumbs_foods
         SET description = 'Temporary update', category = 'Temp'
         WHERE id = 1",
        )
        .execute(&mut *update)
        .await?;
        query(
            "DELETE FROM crumbs_food_tags
         WHERE food_id = 1
           AND tag_id = (SELECT id FROM crumbs_tags WHERE name_key = 'keep')",
        )
        .execute(&mut *update)
        .await?;
        query("INSERT INTO crumbs_tags (name, name_key, created_at) VALUES ('temp', 'temp', 0)")
            .execute(&mut *update)
            .await?;
        query(
            "INSERT INTO crumbs_food_tags (food_id, tag_id, created_at)
         SELECT 1, id, 0 FROM crumbs_tags WHERE name_key = 'temp'",
        )
        .execute(&mut *update)
        .await?;
        assert_matches_on(&mut update, "temporary", &[1]).await?;
        assert_matches_on(&mut update, "baseline", &[]).await?;
        assert_matches_on(&mut update, "tags:temp", &[1]).await?;
        assert_matches_on(&mut update, "tags:keep", &[]).await?;
        update.rollback().await?;
        assert_matches(&database, "temporary", &[]).await?;
        assert_matches(&database, "baseline", &[1]).await?;
        assert_matches(&database, "tags:temp", &[]).await?;
        assert_matches(&database, "tags:keep", &[1]).await?;
        assert_eq!(description(&database, 1).await?, "Committed baseline");
        assert_fts_integrity(&database).await?;

        let keep_tag_id = tag_id(&database, "keep").await?;
        let mut tag_update = database.pool.begin().await?;
        query("UPDATE crumbs_tags SET name = 'renamed', name_key = 'renamed' WHERE id = ?")
            .bind(keep_tag_id)
            .execute(&mut *tag_update)
            .await?;
        query("UPDATE crumbs_food_tags SET food_id = 2 WHERE food_id = 1 AND tag_id = ?")
            .bind(keep_tag_id)
            .execute(&mut *tag_update)
            .await?;
        assert_matches_on(&mut tag_update, "tags:keep", &[]).await?;
        assert_matches_on(&mut tag_update, "tags:renamed", &[2]).await?;
        tag_update.rollback().await?;
        assert_matches(&database, "tags:keep", &[1]).await?;
        assert_matches(&database, "tags:renamed", &[]).await?;
        assert_matches(&database, "tags:retained", &[2]).await?;
        assert_fts_integrity(&database).await?;

        let mut insert = database.pool.begin().await?;
        query(
            "INSERT INTO crumbs_foods
            (id, source, source_food_id, dataset, description, category, parsed_data, created_at)
         VALUES (3, 'usda', 'rolled-insert', 'common', 'Rolled back insert', NULL, '{}', 0)",
        )
        .execute(&mut *insert)
        .await?;
        assert_matches_on(&mut insert, "rolled", &[3]).await?;
        insert.rollback().await?;
        assert_matches(&database, "rolled", &[]).await?;
        assert_eq!(fts_row_count(&database).await?, 2);
        assert_fts_integrity(&database).await?;
        Ok(())
    }

    #[tokio::test]
    async fn backfill_populates_index_and_repeated_open_is_safe() -> Result<(), Box<dyn StdError>> {
        let directory = tempfile::tempdir()?;
        let migrations = directory.path().join("migrations");
        fs::create_dir(&migrations)?;
        fs::write(
            migrations.join("0001_create_crumbs_sources.sql"),
            MIGRATION_0001,
        )?;
        fs::write(
            migrations.join("0002_create_crumbs_foods.sql"),
            MIGRATION_0002,
        )?;
        let migrator = Migrator::new(migrations.as_path()).await?;
        let database_path = directory.path().join("crumbs.db");
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        migrator.run(&pool).await?;
        query("INSERT INTO crumbs_sources (id, created_at) VALUES ('usda', 0)")
            .execute(&pool)
            .await?;
        query(
            "INSERT INTO crumbs_foods
            (id, source, source_food_id, dataset, description, category, parsed_data, created_at)
         VALUES
            (1, 'usda', 'old-1', 'usda_foundation', 'Roșii coapte', 'Legume', '{}', 0),
            (2, 'usda', 'old-2', 'usda_foundation', 'Cheddar Cheese', 'Dairy', '{}', 0),
            (3, 'usda', 'old-3', 'usda_sr_legacy', 'Legacy apple', 'Fruit', '{}', 0),
            (4, 'usda', 'old-4', 'usda_survey_fndds', 'Survey oats', 'Grains', '{}', 0),
            (5, 'usda', 'old-5', 'usda_branded', 'Brand cereal', 'Cereal', '{}', 0)",
        )
        .execute(&pool)
        .await?;
        query(
            "INSERT INTO crumbs_tags (id, name, name_key, created_at)
         VALUES (1, 'roșu', ?, 0), (2, 'sharp', 'sharp', 0)",
        )
        .bind("ros\u{0326}u")
        .execute(&pool)
        .await?;
        query(
            "INSERT INTO crumbs_food_tags (food_id, tag_id, created_at)
         VALUES (1, 1, 0), (2, 2, 0)",
        )
        .execute(&pool)
        .await?;
        pool.close().await;

        let database = Database::open(DatabaseConfig {
            path: database_path.clone(),
        })
        .await?;
        assert_eq!(migration_count(&database).await?, 6);
        assert_matches(&database, "rosii", &[1]).await?;
        assert_matches(&database, "cheddar", &[2]).await?;
        assert_matches(&database, "dairy", &[2]).await?;
        assert_matches(&database, "sharp", &[2]).await?;
        assert_search_matches(&database, "dataset:common", &[1, 2, 3, 4]).await?;
        assert_search_matches(&database, "dataset:branded", &[5]).await?;
        assert_search_matches(&database, "source:usda", &[1, 2, 3, 4, 5]).await?;
        assert_search_matches(&database, "source:usda AND dataset:common", &[1, 2, 3, 4]).await?;
        assert_search_matches(&database, "SOURCE:usda", &[1, 2, 3, 4, 5]).await?;
        assert_eq!(dataset(&database, 1).await?, "common");
        assert_eq!(dataset(&database, 2).await?, "common");
        assert_eq!(dataset(&database, 3).await?, "common");
        assert_eq!(dataset(&database, 4).await?, "common");
        assert_eq!(dataset(&database, 5).await?, "branded");
        assert_eq!(notes(&database, 1).await?, None);
        assert_eq!(fts_row_count(&database).await?, 5);
        assert_eq!(description(&database, 1).await?, "Roșii coapte");
        assert_fts_integrity(&database).await?;
        database.close().await;

        let reopened = Database::open(DatabaseConfig {
            path: database_path,
        })
        .await?;
        assert_eq!(migration_count(&reopened).await?, 6);
        assert_matches(&reopened, "rosii", &[1]).await?;
        assert_matches(&reopened, "cheddar", &[2]).await?;
        assert_search_matches(&reopened, "dataset:common", &[1, 2, 3, 4]).await?;
        assert_search_matches(&reopened, "dataset:branded", &[5]).await?;
        assert_eq!(fts_row_count(&reopened).await?, 5);
        assert_fts_integrity(&reopened).await?;
        Ok(())
    }

    #[tokio::test]
    async fn unknown_legacy_dataset_rolls_back_dataset_and_notes_migration()
    -> Result<(), Box<dyn StdError>> {
        let directory = tempfile::tempdir()?;
        let migrations = directory.path().join("migrations");
        fs::create_dir(&migrations)?;
        fs::write(
            migrations.join("0001_create_crumbs_sources.sql"),
            MIGRATION_0001,
        )?;
        fs::write(
            migrations.join("0002_create_crumbs_foods.sql"),
            MIGRATION_0002,
        )?;
        let migrator = Migrator::new(migrations.as_path()).await?;
        let database_path = directory.path().join("crumbs.db");
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        migrator.run(&pool).await?;
        query("INSERT INTO crumbs_sources (id, created_at) VALUES ('usda', 0)")
            .execute(&pool)
            .await?;
        query(
            "INSERT INTO crumbs_foods
            (id, source, source_food_id, dataset, description, category, parsed_data, created_at)
         VALUES
            (1, 'usda', 'known', 'usda_foundation', 'Known legacy food', NULL, '{}', 0),
            (2, 'usda', 'unknown', 'future_dataset', 'Future food', NULL, '{}', 0)",
        )
        .execute(&pool)
        .await?;
        query(
            "INSERT INTO crumbs_tags (id, name, name_key, created_at)
         VALUES (1, 'migration tag', 'migration tag', 0)",
        )
        .execute(&pool)
        .await?;
        query(
            "INSERT INTO crumbs_food_tags (food_id, tag_id, created_at)
         VALUES (1, 1, 0), (2, 1, 0)",
        )
        .execute(&pool)
        .await?;
        pool.close().await;

        let migration_error = match Database::open(DatabaseConfig {
            path: database_path.clone(),
        })
        .await
        {
            Err(crate::Error::Migration { source, .. }) => source.to_string(),
            Err(error) => return Err(error.into()),
            Ok(database) => {
                database.close().await;
                return Err("unknown dataset unexpectedly passed migration".into());
            }
        };
        assert!(
            migration_error.contains("foods_dataset_must_be_common_branded_or_custom"),
            "{migration_error}"
        );

        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        let datasets: Vec<(FoodId, DatasetText)> =
            query_as("SELECT id, dataset FROM crumbs_foods ORDER BY id")
                .fetch_all(&pool)
                .await?;
        assert_eq!(
            datasets,
            vec![
                (1, "usda_foundation".to_owned()),
                (2, "future_dataset".to_owned()),
            ]
        );
        let notes_column_count: FoodCount = query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('crumbs_foods') WHERE name = 'notes'",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(notes_column_count, 0);
        let tag_assignment_count: FoodCount = query_scalar("SELECT COUNT(*) FROM crumbs_food_tags")
            .fetch_one(&pool)
            .await?;
        assert_eq!(tag_assignment_count, 2);
        let migration_count: MigrationCount =
            query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&pool)
                .await?;
        assert_eq!(migration_count, 5);
        let indexed_ids: Vec<FoodId> = query_scalar(
            "SELECT rowid FROM crumbs_foods_fts
         WHERE crumbs_foods_fts MATCH 'migration tag'
         ORDER BY rowid",
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(indexed_ids, vec![1, 2]);
        pool.close().await;
        Ok(())
    }

    async fn insert_food(
        database: &Database,
        id: FoodId,
        source: Source<'_>,
        source_food_id: SourceFoodId<'_>,
        description: RawFoodDescription<'_>,
        category: Option<RawFoodCategory<'_>>,
        tags: RawTagName<'_>,
    ) -> Result<(), Box<dyn StdError>> {
        query("INSERT OR IGNORE INTO crumbs_sources (id, created_at) VALUES (?, 0)")
            .bind(source)
            .execute(&database.pool)
            .await?;
        query(
            "INSERT INTO crumbs_foods
            (id, source, source_food_id, dataset, description, category, parsed_data, created_at)
         VALUES (?, ?, ?, 'common', ?, ?, '{}', 0)",
        )
        .bind(id)
        .bind(source)
        .bind(source_food_id)
        .bind(description)
        .bind(category)
        .execute(&database.pool)
        .await?;
        if !tags.is_empty() {
            let tag = TagName::new(tags)?;
            database
                .set_food_tags(crate::foods::FoodId(id), &[tag])
                .await?;
        }
        Ok(())
    }

    async fn description(database: &Database, id: FoodId) -> Result<FoodDescription, sqlx::Error> {
        query_scalar("SELECT description FROM crumbs_foods WHERE id = ?")
            .bind(id)
            .fetch_one(&database.pool)
            .await
    }

    async fn dataset(database: &Database, id: FoodId) -> Result<DatasetText, sqlx::Error> {
        query_scalar("SELECT dataset FROM crumbs_foods WHERE id = ?")
            .bind(id)
            .fetch_one(&database.pool)
            .await
    }

    async fn notes(database: &Database, id: FoodId) -> Result<Option<FoodNotes>, sqlx::Error> {
        query_scalar("SELECT notes FROM crumbs_foods WHERE id = ?")
            .bind(id)
            .fetch_one(&database.pool)
            .await
    }

    async fn category(
        database: &Database,
        id: FoodId,
    ) -> Result<Option<FoodCategory>, sqlx::Error> {
        query_scalar("SELECT category FROM crumbs_foods WHERE id = ?")
            .bind(id)
            .fetch_one(&database.pool)
            .await
    }

    async fn tags(database: &Database, id: FoodId) -> Result<IndexedTags, sqlx::Error> {
        query_scalar("SELECT tags FROM crumbs_foods_search_content WHERE id = ?")
            .bind(id)
            .fetch_one(&database.pool)
            .await
    }

    async fn tag_id(database: &Database, name_key: TagKey<'_>) -> Result<TagId, sqlx::Error> {
        query_scalar("SELECT id FROM crumbs_tags WHERE name_key = ?")
            .bind(name_key)
            .fetch_one(&database.pool)
            .await
    }

    async fn matching_ids(
        database: &Database,
        query: FtsQuery<'_>,
    ) -> Result<Vec<FoodId>, sqlx::Error> {
        query_scalar(
            "SELECT f.id
         FROM crumbs_foods_fts
         JOIN crumbs_foods AS f ON f.id = crumbs_foods_fts.rowid
         WHERE crumbs_foods_fts MATCH ?
         ORDER BY f.id",
        )
        .bind(query)
        .fetch_all(&database.pool)
        .await
    }

    async fn assert_matches(
        database: &Database,
        query: FtsQuery<'_>,
        expected: &[FoodId],
    ) -> Result<(), Box<dyn StdError>> {
        let actual = matching_ids(database, query).await?;
        assert_eq!(actual, expected, "MATCH {query:?}");
        Ok(())
    }

    async fn assert_search_matches(
        database: &Database,
        expression: FtsQuery<'_>,
        expected: &[FoodId],
    ) -> Result<(), Box<dyn StdError>> {
        let query = SearchQuery::new(expression)?;
        let results = database.search_foods(&query, FoodLimit::default()).await?;
        let mut actual = results
            .common
            .into_iter()
            .chain(results.branded)
            .chain(results.custom)
            .map(|result| result.id.0)
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected, "search {expression:?}");
        Ok(())
    }

    async fn assert_matches_on(
        connection: &mut SqliteConnection,
        query: FtsQuery<'_>,
        expected: &[FoodId],
    ) -> Result<(), Box<dyn StdError>> {
        let actual: Vec<FoodId> = query_scalar(
            "SELECT f.id
         FROM crumbs_foods_fts
         JOIN crumbs_foods AS f ON f.id = crumbs_foods_fts.rowid
         WHERE crumbs_foods_fts MATCH ?
         ORDER BY f.id",
        )
        .bind(query)
        .fetch_all(&mut *connection)
        .await?;
        assert_eq!(actual, expected, "MATCH {query:?} inside transaction");
        Ok(())
    }

    async fn fts_row_count(database: &Database) -> Result<FoodCount, sqlx::Error> {
        query_scalar("SELECT COUNT(*) FROM crumbs_foods_fts")
            .fetch_one(&database.pool)
            .await
    }

    async fn migration_count(database: &Database) -> Result<MigrationCount, sqlx::Error> {
        query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&database.pool)
            .await
    }

    async fn indexed_rowid(
        database: &Database,
        query: FtsQuery<'_>,
    ) -> Result<Option<FoodId>, sqlx::Error> {
        query_scalar("SELECT rowid FROM crumbs_foods_fts WHERE crumbs_foods_fts MATCH ?")
            .bind(query)
            .fetch_optional(&database.pool)
            .await
    }

    async fn indexed_rowid_count(
        database: &Database,
        rowid: FoodId,
    ) -> Result<FoodCount, sqlx::Error> {
        query_scalar("SELECT COUNT(*) FROM crumbs_foods_fts WHERE rowid = ?")
            .bind(rowid)
            .fetch_one(&database.pool)
            .await
    }

    async fn vocab_terms(database: &Database) -> Result<Vec<FtsTerm>, Box<dyn StdError>> {
        query("CREATE VIRTUAL TABLE fts_vocab USING fts5vocab(crumbs_foods_fts, 'row')")
            .execute(&database.pool)
            .await?;
        let terms = query_scalar("SELECT term FROM fts_vocab ORDER BY term")
            .fetch_all(&database.pool)
            .await?;
        query("DROP TABLE fts_vocab")
            .execute(&database.pool)
            .await?;
        Ok(terms)
    }

    async fn assert_fts_integrity(database: &Database) -> Result<(), Box<dyn StdError>> {
        query(
            "INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rank)
         VALUES ('integrity-check', 1)",
        )
        .execute(&database.pool)
        .await?;
        let integrity: String = query_scalar("PRAGMA integrity_check")
            .fetch_one(&database.pool)
            .await?;
        assert_eq!(integrity, "ok");
        Ok(())
    }
}
