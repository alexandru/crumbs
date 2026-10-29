use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::{SqliteConnection, query, query_scalar};

use crate::{Database, DatabaseConfig};

const FOUNDATION_JSON: &str = include_str!("../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../test-fixtures/branded.json");
const MIGRATION_0001: &str = include_str!("../migrations/0001_create_crumbs_sources.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_create_crumbs_foods.sql");

#[tokio::test]
async fn schema_uses_external_content_unicode_tokenizer_and_triggers()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;

    let sql: String = query_scalar("SELECT sql FROM sqlite_master WHERE name = 'crumbs_foods_fts'")
        .fetch_one(&database.pool)
        .await?;
    assert!(sql.contains("content = 'crumbs_foods'"), "{sql}");
    assert!(sql.contains("content_rowid = 'id'"), "{sql}");
    assert!(sql.contains("remove_diacritics 2"), "{sql}");

    let triggers: Vec<String> = query_scalar(
        "SELECT name FROM sqlite_master
         WHERE type = 'trigger' AND name LIKE 'crumbs_foods_fts_after_%'
         ORDER BY name",
    )
    .fetch_all(&database.pool)
    .await?;
    assert_eq!(
        triggers,
        vec![
            "crumbs_foods_fts_after_delete",
            "crumbs_foods_fts_after_insert",
            "crumbs_foods_fts_after_update",
        ]
    );
    assert_eq!(fts_row_count(&database).await?, 0);
    assert_fts_integrity(&database).await?;
    Ok(())
}

#[tokio::test]
async fn imported_fixtures_are_searchable_and_preserve_display_text()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let foundation = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    let branded = write_input(directory.path(), "branded.json", BRANDED_JSON)?;

    database.import_usda(&foundation).await?;
    database.import_usda(&branded).await?;

    let hummus = food_id(&database, "usda", "321358").await?;
    let tomatoes = food_id(&database, "usda", "321360").await?;
    let granola = food_id(&database, "usda", "1106281").await?;

    assert_matches(&database, "hummus", &[hummus]).await?;
    assert_matches(&database, "legumes", &[hummus]).await?;
    assert_matches(&database, "category:legumes", &[hummus]).await?;
    assert_matches(&database, "tomatoes", &[tomatoes]).await?;
    assert_matches(&database, "vegetables", &[tomatoes]).await?;
    assert_matches(&database, "description:tomatoes", &[tomatoes]).await?;
    assert_matches(&database, "granola", &[granola]).await?;
    assert_matches(&database, "cereal", &[granola]).await?;
    assert_eq!(description(&database, hummus).await?, "Hummus, commercial");
    assert_eq!(
        description(&database, granola).await?,
        "GRANOLA, CINNAMON, RAISIN"
    );
    assert_eq!(
        category(&database, hummus).await?.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert_eq!(
        category(&database, granola).await?.as_deref(),
        Some("Cereal")
    );
    assert_fts_integrity(&database).await?;
    Ok(())
}

#[tokio::test]
async fn english_matching_is_case_insensitive_without_stemming() -> Result<(), Box<dyn StdError>> {
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
async fn romanian_diacritics_match_english_case_and_all_variants() -> Result<(), Box<dyn StdError>>
{
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
async fn decomposed_romanian_forms_match_composed_and_plain_forms() -> Result<(), Box<dyn StdError>>
{
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
async fn reimport_replaces_stale_terms_without_duplicate_hits() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let foundation = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    database.import_usda(&foundation).await?;
    let id = food_id(&database, "usda", "321358").await?;

    let updated = write_input(
        directory.path(),
        "updated.json",
        r#"{"FoundationFoods":[{"fdcId":321358,"description":"Chickpea spread","dataType":"Foundation","foodCategory":{"description":"Spreads"}}]}"#,
    )?;
    database.import_usda(&updated).await?;

    assert_eq!(food_id(&database, "usda", "321358").await?, id);
    assert_eq!(description(&database, id).await?, "Chickpea spread");
    for stale in ["hummus", "commercial", "legumes"] {
        assert_matches(&database, stale, &[]).await?;
    }
    assert_matches(&database, "chickpea", &[id]).await?;
    assert_matches(&database, "spreads", &[id]).await?;
    assert_eq!(fts_row_count(&database).await?, 2);

    for _ in 0..3 {
        database.import_usda(&updated).await?;
    }
    assert_matches(&database, "chickpea", &[id]).await?;
    assert_eq!(fts_row_count(&database).await?, 2);
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
         SET description = 'Changed food', category = 'Newcat', tags = 'newtag'
         WHERE id = 1",
    )
    .execute(&database.pool)
    .await?;
    assert_matches(&database, "mutable", &[]).await?;
    assert_matches(&database, "oldcat", &[]).await?;
    assert_matches(&database, "oldtag", &[]).await?;
    assert_matches(&database, "changed", &[1]).await?;
    assert_matches(&database, "newcat", &[1]).await?;
    assert_matches(&database, "newtag", &[1]).await?;

    query("UPDATE crumbs_foods SET category = NULL, tags = '' WHERE id = 1")
        .execute(&database.pool)
        .await?;
    assert_matches(&database, "newcat", &[]).await?;
    assert_matches(&database, "newtag", &[]).await?;
    assert_matches(&database, "changed", &[1]).await?;
    assert_eq!(category(&database, 1).await?, None);
    assert_eq!(tags(&database, 1).await?, "");

    query("UPDATE crumbs_foods SET description = 'Changed food', category = NULL, tags = '' WHERE id = 1")
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
         SET description = 'Changed Other', category = 'Other Changed', tags = 'changedtag'
         WHERE source = 'other' AND id = 2",
    )
    .execute(&database.pool)
    .await?;
    assert_matches(&database, "shared", &[1]).await?;
    assert_matches(&database, "changed", &[2]).await?;
    assert_eq!(description(&database, 1).await?, "Shared Orchard Apple");
    assert_fts_integrity(&database).await?;
    Ok(())
}

#[tokio::test]
async fn direct_rowid_update_and_delete_keep_index_consistent() -> Result<(), Box<dyn StdError>> {
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
    assert_eq!(indexed_rowid(&database, "alpha").await?, Some(20));
    assert_eq!(indexed_rowid_count(&database, 2).await?, 0);

    query(
        "UPDATE crumbs_foods
         SET id = 30, description = 'Direct beta', category = NULL, tags = 'tag2'
         WHERE id = 20",
    )
    .execute(&database.pool)
    .await?;
    assert_matches(&database, "alpha", &[]).await?;
    assert_matches(&database, "beta", &[30]).await?;
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

    let mut update = database.pool.begin().await?;
    query(
        "UPDATE crumbs_foods
         SET description = 'Temporary update', category = 'Temp', tags = 'temp'
         WHERE id = 1",
    )
    .execute(&mut *update)
    .await?;
    assert_matches_on(&mut update, "temporary", &[1]).await?;
    assert_matches_on(&mut update, "baseline", &[]).await?;
    update.rollback().await?;
    assert_matches(&database, "temporary", &[]).await?;
    assert_matches(&database, "baseline", &[1]).await?;
    assert_eq!(description(&database, 1).await?, "Committed baseline");

    let mut insert = database.pool.begin().await?;
    query(
        "INSERT INTO crumbs_foods
            (id, source, source_food_id, food_type, description, category, tags, source_data)
         VALUES (2, 'usda', 'rolled-insert', 'Foundation', 'Rolled back insert', NULL, '', '{}')",
    )
    .execute(&mut *insert)
    .await?;
    assert_matches_on(&mut insert, "rolled", &[2]).await?;
    insert.rollback().await?;
    assert_matches(&database, "rolled", &[]).await?;
    assert_eq!(fts_row_count(&database).await?, 1);
    assert_fts_integrity(&database).await?;
    Ok(())
}

#[tokio::test]
async fn import_failure_keeps_earlier_food_searchable_and_retry_succeeds()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(
        directory.path(),
        "partial.json",
        r#"{"FoundationFoods":[
            {"fdcId":9401,"description":"Searchable first food","dataType":"Foundation"},
            {"fdcId":9402,"description":"  ","dataType":"Foundation"}
        ]}"#,
    )?;

    assert!(database.import_usda(&path).await.is_err());
    let first = food_id(&database, "usda", "9401").await?;
    assert_matches(&database, "searchable", &[first]).await?;
    assert_matches(&database, "second", &[]).await?;

    fs::write(
        &path,
        r#"{"FoundationFoods":[
            {"fdcId":9401,"description":"Searchable first food","dataType":"Foundation"},
            {"fdcId":9402,"description":"Second food","dataType":"Foundation"}
        ]}"#,
    )?;
    database.import_usda(&path).await?;
    let second = food_id(&database, "usda", "9402").await?;
    assert_matches(&database, "searchable", &[first]).await?;
    assert_matches(&database, "second", &[second]).await?;
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
    query(
        "INSERT INTO crumbs_foods
            (id, source, source_food_id, food_type, description, category, tags, source_data)
         VALUES
            (1, 'usda', 'old-1', 'Foundation', 'Roșii coapte', 'Legume', 'roșu', '{}'),
            (2, 'usda', 'old-2', 'Foundation', 'Cheddar Cheese', 'Dairy', 'sharp', '{}')",
    )
    .execute(&pool)
    .await?;
    pool.close().await;

    let database = Database::open(DatabaseConfig {
        path: database_path.clone(),
    })
    .await?;
    assert_eq!(migration_count(&database).await?, 3);
    assert_matches(&database, "rosii", &[1]).await?;
    assert_matches(&database, "cheddar", &[2]).await?;
    assert_matches(&database, "dairy", &[2]).await?;
    assert_matches(&database, "sharp", &[2]).await?;
    assert_eq!(fts_row_count(&database).await?, 2);
    assert_eq!(description(&database, 1).await?, "Roșii coapte");
    assert_fts_integrity(&database).await?;
    database.close().await;

    let reopened = Database::open(DatabaseConfig {
        path: database_path,
    })
    .await?;
    assert_eq!(migration_count(&reopened).await?, 3);
    assert_matches(&reopened, "rosii", &[1]).await?;
    assert_matches(&reopened, "cheddar", &[2]).await?;
    assert_eq!(fts_row_count(&reopened).await?, 2);
    assert_fts_integrity(&reopened).await?;
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

fn write_input(directory: &Path, name: &str, contents: &str) -> Result<PathBuf, Box<dyn StdError>> {
    let path = directory.join(name);
    fs::write(&path, contents)?;
    Ok(path)
}

async fn insert_food(
    database: &Database,
    id: i64,
    source: &str,
    source_food_id: &str,
    description: &str,
    category: Option<&str>,
    tags: &str,
) -> Result<(), Box<dyn StdError>> {
    query("INSERT OR IGNORE INTO crumbs_sources (id) VALUES (?)")
        .bind(source)
        .execute(&database.pool)
        .await?;
    query(
        "INSERT INTO crumbs_foods
            (id, source, source_food_id, food_type, description, category, tags, source_data)
         VALUES (?, ?, ?, 'Foundation', ?, ?, ?, '{}')",
    )
    .bind(id)
    .bind(source)
    .bind(source_food_id)
    .bind(description)
    .bind(category)
    .bind(tags)
    .execute(&database.pool)
    .await?;
    Ok(())
}

async fn food_id(
    database: &Database,
    source: &str,
    source_food_id: &str,
) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT id FROM crumbs_foods WHERE source = ? AND source_food_id = ?")
        .bind(source)
        .bind(source_food_id)
        .fetch_one(&database.pool)
        .await
}

async fn description(database: &Database, id: i64) -> Result<String, sqlx::Error> {
    query_scalar("SELECT description FROM crumbs_foods WHERE id = ?")
        .bind(id)
        .fetch_one(&database.pool)
        .await
}

async fn category(database: &Database, id: i64) -> Result<Option<String>, sqlx::Error> {
    query_scalar("SELECT category FROM crumbs_foods WHERE id = ?")
        .bind(id)
        .fetch_one(&database.pool)
        .await
}

async fn tags(database: &Database, id: i64) -> Result<String, sqlx::Error> {
    query_scalar("SELECT tags FROM crumbs_foods WHERE id = ?")
        .bind(id)
        .fetch_one(&database.pool)
        .await
}

async fn matching_ids(database: &Database, query: &str) -> Result<Vec<i64>, sqlx::Error> {
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
    query: &str,
    expected: &[i64],
) -> Result<(), Box<dyn StdError>> {
    let actual = matching_ids(database, query).await?;
    assert_eq!(actual, expected, "MATCH {query:?}");
    Ok(())
}

async fn assert_matches_on(
    connection: &mut SqliteConnection,
    query: &str,
    expected: &[i64],
) -> Result<(), Box<dyn StdError>> {
    let actual: Vec<i64> = query_scalar(
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

async fn fts_row_count(database: &Database) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM crumbs_foods_fts")
        .fetch_one(&database.pool)
        .await
}

async fn migration_count(database: &Database) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(&database.pool)
        .await
}

async fn indexed_rowid(database: &Database, query: &str) -> Result<Option<i64>, sqlx::Error> {
    query_scalar("SELECT rowid FROM crumbs_foods_fts WHERE crumbs_foods_fts MATCH ?")
        .bind(query)
        .fetch_optional(&database.pool)
        .await
}

async fn indexed_rowid_count(database: &Database, rowid: i64) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM crumbs_foods_fts WHERE rowid = ?")
        .bind(rowid)
        .fetch_one(&database.pool)
        .await
}

async fn vocab_terms(database: &Database) -> Result<Vec<String>, Box<dyn StdError>> {
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
    query("INSERT INTO crumbs_foods_fts (crumbs_foods_fts) VALUES ('integrity-check')")
        .execute(&database.pool)
        .await?;
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
