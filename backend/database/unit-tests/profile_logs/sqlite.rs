use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rust_decimal::Decimal;
use sqlx::FromRow;
use tokio::sync::Barrier;

use super::open_database;
use crate::identity::UserId;
use crate::profile_logs::*;
use crate::profiles::{CreateProfile, Profile, ProfileId, Profiles};
use crate::runtime::Database;

async fn insert_user(database: &Database, id: i64) -> Result<UserId, Box<dyn Error>> {
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, active, created_at)
         VALUES (?, ?, 'hash', 1, 1)",
    )
    .bind(id)
    .bind(format!("user{id}"))
    .execute(&database.pool)
    .await?;
    Ok(UserId::new(id)?)
}

async fn create_profile(
    database: &Database,
    user: UserId,
    input: serde_json::Value,
) -> Result<Profile, Box<dyn Error>> {
    let input: CreateProfile = serde_json::from_value(input)?;
    database
        .create_profile(user, &input)
        .await
        .map_err(Into::into)
}

async fn create_weight_log(
    database: &Database,
    user: UserId,
    profile: ProfileId,
    day: &str,
    time: Option<&str>,
    weight: &str,
) -> Result<ProfileLog, Box<dyn Error>> {
    let mut input = serde_json::json!({
        "day": day,
        "entry": {
            "type": "biometric",
            "measurement": {
                "type": "weight",
                "value": weight
            }
        }
    });
    if let Some(time) = time {
        let Some(fields) = input.as_object_mut() else {
            return Err(std::io::Error::other("test create input was not an object").into());
        };
        fields.insert("time".to_owned(), serde_json::json!(time));
    }
    let input: CreateLog = serde_json::from_value(input)?;
    database
        .create_profile_log(user, profile, &input)
        .await?
        .ok_or_else(|| std::io::Error::other("owned profile was not found").into())
}

async fn get_profile(
    database: &Database,
    user: UserId,
    profile: ProfileId,
) -> Result<Profile, Box<dyn Error>> {
    database
        .profile(user, profile)
        .await?
        .ok_or_else(|| std::io::Error::other("owned profile was not found").into())
}

fn current_weight(profile: &Profile) -> Option<Decimal> {
    profile
        .data
        .cache
        .current_weight
        .map(|weight| weight.value().value())
}

#[derive(FromRow)]
struct LogState {
    day: String,
    time: String,
    data_json: String,
    created_at: i64,
    updated_at: Option<i64>,
}

async fn log_state(database: &Database, id: ProfileLogId) -> Result<LogState, sqlx::Error> {
    sqlx::query_as::<_, LogState>(
        "SELECT day, time, data_json, created_at, updated_at
         FROM profile_logs WHERE id = ?",
    )
    .bind(id.get())
    .fetch_one(&database.pool)
    .await
}

#[derive(FromRow)]
struct ProfileState {
    created_at: i64,
    updated_at: Option<i64>,
}

async fn profile_state(database: &Database, id: ProfileId) -> Result<ProfileState, sqlx::Error> {
    sqlx::query_as::<_, ProfileState>("SELECT created_at, updated_at FROM profiles WHERE id = ?")
        .bind(id.get())
        .fetch_one(&database.pool)
        .await
}

#[tokio::test]
async fn omitted_time_uses_profile_local_clock_but_preserves_backdated_day()
-> Result<(), Box<dyn Error>> {
    let instant =
        chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05.123Z")?.timestamp_millis();
    let (_directory, database, _) = open_database(u64::try_from(instant)?).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(
        &database,
        user,
        serde_json::json!({
            "name": "Alex",
            "settings": {"timezone": "America/Los_Angeles"}
        }),
    )
    .await?;

    let log = create_weight_log(&database, user, profile.id, "1982-09-10", None, "70 kg").await?;
    assert_eq!(log.day, LogDay::parse("1982-09-10")?);
    assert_eq!(log.time.to_string(), "19:04:05.123");
    let state = log_state(&database, log.id).await?;
    assert_eq!(state.day, "1982-09-10");
    assert_eq!(state.time, "19:04:05.123");
    assert_eq!(state.created_at, instant);
    assert_eq!(state.updated_at, None);
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(70))
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn ownership_patch_get_delete_and_cache_are_scoped_to_profile() -> Result<(), Box<dyn Error>>
{
    let (_directory, database, clock) = open_database(1_000).await?;
    let alex = insert_user(&database, 1).await?;
    let cristi = insert_user(&database, 2).await?;
    let alex_profile = create_profile(&database, alex, serde_json::json!({"name":"Alex"})).await?;
    let cristi_profile =
        create_profile(&database, cristi, serde_json::json!({"name":"Alex"})).await?;
    let original = create_weight_log(
        &database,
        alex,
        alex_profile.id,
        "2024-01-01",
        Some("12:00"),
        "70 kg",
    )
    .await?;

    assert_eq!(
        database
            .profile_log(cristi, alex_profile.id, original.id)
            .await?,
        None
    );
    assert_eq!(
        database
            .list_profile_logs(cristi, alex_profile.id, &LogFilter::default())
            .await?,
        None
    );
    let other_profile_patch: PatchLog = serde_json::from_str(r#"{"day":"2025-01-01"}"#)?;
    assert_eq!(
        database
            .patch_profile_log(cristi, cristi_profile.id, original.id, &other_profile_patch)
            .await?,
        None
    );
    assert!(
        !database
            .delete_profile_log(cristi, alex_profile.id, original.id)
            .await?
    );
    assert_eq!(
        database
            .profile_log(alex, alex_profile.id, original.id)
            .await?,
        Some(original)
    );

    let original_profile_state = profile_state(&database, alex_profile.id).await?;
    assert_eq!(original_profile_state.created_at, 1_000);
    let empty_patch = PatchLog::default();
    let unchanged = database
        .patch_profile_log(alex, alex_profile.id, original.id, &empty_patch)
        .await?
        .ok_or_else(|| std::io::Error::other("owned log was not found"))?;
    assert_eq!(unchanged, original);
    assert_eq!(log_state(&database, original.id).await?.updated_at, None);
    assert_eq!(
        profile_state(&database, alex_profile.id).await?.updated_at,
        original_profile_state.updated_at
    );

    clock.store(u64::MAX, Ordering::SeqCst);
    let same_patch: PatchLog = serde_json::from_str(
        r#"{"day":"2024-01-01","time":"12:00","entry":{"type":"biometric","measurement":{"type":"weight","value":"70 kg"}}}"#,
    )?;
    assert_eq!(
        database
            .patch_profile_log(alex, alex_profile.id, original.id, &same_patch)
            .await?,
        Some(original)
    );
    assert_eq!(log_state(&database, original.id).await?.updated_at, None);
    assert_eq!(
        profile_state(&database, alex_profile.id).await?.updated_at,
        original_profile_state.updated_at
    );

    clock.store(2_000, Ordering::SeqCst);
    let date_time_patch: PatchLog = serde_json::from_str(r#"{"day":"2024-02-02","time":"13:14"}"#)?;
    let changed = database
        .patch_profile_log(alex, alex_profile.id, original.id, &date_time_patch)
        .await?
        .ok_or_else(|| std::io::Error::other("owned log was not found"))?;
    assert_eq!(changed.day, LogDay::parse("2024-02-02")?);
    assert_eq!(changed.time.to_string(), "13:14:00.000");
    assert_eq!(changed.data, original.data);
    let state = log_state(&database, original.id).await?;
    assert_eq!(state.created_at, 1_000);
    assert_eq!(state.updated_at, Some(2_000));
    assert_eq!(
        profile_state(&database, alex_profile.id).await?.updated_at,
        original_profile_state.updated_at
    );
    assert_eq!(
        current_weight(&get_profile(&database, alex, alex_profile.id).await?),
        Some(Decimal::from(70))
    );

    clock.store(3_000, Ordering::SeqCst);
    let value_patch: PatchLog = serde_json::from_str(
        r#"{"entry":{"type":"biometric","measurement":{"type":"weight","value":"75 kg"}}}"#,
    )?;
    let updated = database
        .patch_profile_log(alex, alex_profile.id, original.id, &value_patch)
        .await?
        .ok_or_else(|| std::io::Error::other("owned log was not found"))?;
    assert_eq!(
        current_weight(&get_profile(&database, alex, alex_profile.id).await?),
        Some(Decimal::from(75))
    );
    assert_eq!(
        log_state(&database, original.id).await?.updated_at,
        Some(3_000)
    );
    assert_eq!(
        serde_json::to_value(updated.data)?,
        serde_json::json!({
            "type": "biometric",
            "measurement": {"type": "weight", "value": "75 kg"}
        })
    );
    assert_eq!(
        profile_state(&database, alex_profile.id).await?.updated_at,
        Some(3_000)
    );

    assert!(serde_json::from_str::<PatchLog>(r#"{"entry":null}"#).is_err());
    assert!(
        database
            .delete_profile_log(alex, alex_profile.id, original.id)
            .await?
    );
    assert_eq!(
        current_weight(&get_profile(&database, alex, alex_profile.id).await?),
        None
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn malformed_stored_log_cannot_be_patched_or_deleted() -> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database(1_000).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(&database, user, serde_json::json!({"name": "Alex"})).await?;
    let log = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-01",
        Some("12:00"),
        "70 kg",
    )
    .await?;
    let invalid = r#"{"type":"biometric","measurement":{"type":"weight","value":"0 kg"}}"#;
    sqlx::query("UPDATE profile_logs SET data_json = ? WHERE id = ?")
        .bind(invalid)
        .bind(log.id.get())
        .execute(&database.pool)
        .await?;
    let before = profile_state(&database, profile.id).await?;
    assert!(matches!(
        database
            .patch_profile_log(user, profile.id, log.id, &PatchLog::default())
            .await,
        Err(ProfileLogError::InvalidStoredData)
    ));
    assert!(matches!(
        database.delete_profile_log(user, profile.id, log.id).await,
        Err(ProfileLogError::InvalidStoredData)
    ));
    let stored = log_state(&database, log.id).await?;
    assert_eq!(stored.data_json, invalid);
    assert_eq!(stored.updated_at, None);
    assert_eq!(
        profile_state(&database, profile.id).await?.updated_at,
        before.updated_at
    );
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(70))
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn weight_order_uses_day_time_created_at_then_id_and_delete_falls_back()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, clock) = open_database(1_000).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(&database, user, serde_json::json!({"name":"Alex"})).await?;

    clock.store(5_000, Ordering::SeqCst);
    let older_day = create_weight_log(
        &database,
        user,
        profile.id,
        "2023-12-31",
        Some("23:59"),
        "60 kg",
    )
    .await?;
    clock.store(1_000, Ordering::SeqCst);
    let earlier_time = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-01",
        Some("12:00"),
        "65 kg",
    )
    .await?;
    clock.store(6_000, Ordering::SeqCst);
    let newer_created_at = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-01",
        Some("13:00"),
        "70 kg",
    )
    .await?;
    let higher_id = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-01",
        Some("13:00"),
        "75 kg",
    )
    .await?;
    clock.store(500, Ordering::SeqCst);
    let later_time = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-01",
        Some("14:00"),
        "80 kg",
    )
    .await?;
    clock.store(99_999, Ordering::SeqCst);
    let earlier_day = create_weight_log(
        &database,
        user,
        profile.id,
        "2023-12-30",
        Some("23:59"),
        "90 kg",
    )
    .await?;

    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(80))
    );
    assert!(
        database
            .delete_profile_log(user, profile.id, later_time.id)
            .await?
    );
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(75))
    );
    assert!(
        database
            .delete_profile_log(user, profile.id, higher_id.id)
            .await?
    );
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(70))
    );

    for log in [newer_created_at, earlier_time, older_day, earlier_day] {
        assert!(
            database
                .delete_profile_log(user, profile.id, log.id)
                .await?
        );
    }
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        None
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn list_uses_inclusive_day_bounds_descending_cursor_and_fixed_100_page()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database(1_000).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(&database, user, serde_json::json!({"name":"Alex"})).await?;
    let day = "2024-01-01";
    let mut ids = Vec::new();
    for _ in 0..101 {
        ids.push(
            create_weight_log(&database, user, profile.id, day, Some("12:00"), "70 kg")
                .await?
                .id,
        );
    }

    let filter = LogFilter::new(
        Some(LogDay::parse(day)?),
        Some(LogDay::parse(day)?),
        Some(LogEntryType::Biometric),
        Some(BiometricType::Weight),
        None,
    )?;
    let first = database
        .list_profile_logs(user, profile.id, &filter)
        .await?
        .ok_or_else(|| std::io::Error::other("owned profile was not found"))?;
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.items.first().map(|log| log.id), ids.last().copied());
    assert_eq!(first.items.last().map(|log| log.id), ids.get(1).copied());
    assert_eq!(first.last_id, ids.get(1).copied());

    let next_filter = LogFilter::new(None, None, None, None, first.last_id)?;
    let next = database
        .list_profile_logs(user, profile.id, &next_filter)
        .await?
        .ok_or_else(|| std::io::Error::other("owned profile was not found"))?;
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.items.first().map(|log| log.id), ids.first().copied());
    assert_eq!(next.last_id, ids.first().copied());

    let food_filter = LogFilter::new(None, None, Some(LogEntryType::Food), None, None)?;
    let food_page = database
        .list_profile_logs(user, profile.id, &food_filter)
        .await?
        .ok_or_else(|| std::io::Error::other("owned profile was not found"))?;
    assert!(food_page.items.is_empty());
    assert_eq!(food_page.last_id, None);
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn cache_recalculation_updates_calculated_bmr_and_preserves_custom_settings()
-> Result<(), Box<dyn Error>> {
    let instant = chrono::DateTime::parse_from_rfc3339("2020-01-01T12:00:00Z")?.timestamp_millis();
    let (_directory, database, clock) = open_database(u64::try_from(instant)?).await?;
    let user = insert_user(&database, 1).await?;
    let calculated = create_profile(
        &database,
        user,
        serde_json::json!({
            "name":"Calculated",
            "settings": {
                "birth_date":"1990-01-01",
                "sex":"male",
                "height":"170 cm",
                "timezone":"UTC"
            }
        }),
    )
    .await?;
    assert_eq!(calculated.derived.bmr(), None);
    create_weight_log(
        &database,
        user,
        calculated.id,
        "2020-01-01",
        Some("12:00"),
        "70 kg",
    )
    .await?;
    assert_eq!(
        get_profile(&database, user, calculated.id)
            .await?
            .derived
            .bmr()
            .map(|value| value.amount().value()),
        Some(Decimal::new(16175, 1))
    );

    let next_day = chrono::DateTime::parse_from_rfc3339("2020-01-02T12:00:00Z")?.timestamp_millis();
    clock.store(u64::try_from(next_day)?, Ordering::SeqCst);
    create_weight_log(
        &database,
        user,
        calculated.id,
        "2020-01-02",
        Some("12:00"),
        "75 kg",
    )
    .await?;
    let calculated_after = get_profile(&database, user, calculated.id).await?;
    assert_eq!(current_weight(&calculated_after), Some(Decimal::from(75)));
    assert_eq!(
        calculated_after
            .derived
            .bmr()
            .map(|value| value.amount().value()),
        Some(Decimal::new(16675, 1))
    );
    assert_eq!(
        calculated_after.data.settings.height,
        calculated.data.settings.height
    );

    let custom = create_profile(
        &database,
        user,
        serde_json::json!({
            "name":"Custom",
            "settings": {
                "bmr":{"mode":"custom","value":"1500 kcal/day"},
                "baseline_activity":{"mode":"custom","value":"100 kcal/day"}
            }
        }),
    )
    .await?;
    create_weight_log(
        &database,
        user,
        custom.id,
        "2020-01-02",
        Some("13:00"),
        "80 kg",
    )
    .await?;
    let custom_after = get_profile(&database, user, custom.id).await?;
    assert_eq!(current_weight(&custom_after), Some(Decimal::from(80)));
    assert_eq!(
        custom_after
            .derived
            .bmr()
            .map(|value| value.amount().value()),
        Some(Decimal::from(1500))
    );
    assert_eq!(
        custom_after
            .derived
            .maintenance_calories()
            .map(|value| value.amount().value()),
        Some(Decimal::from(1600))
    );
    assert_eq!(custom_after.data.settings.bmr, custom.data.settings.bmr);
    assert_eq!(
        custom_after.data.settings.baseline_activity,
        custom.data.settings.baseline_activity
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn failed_log_and_cache_mutations_roll_back_together() -> Result<(), Box<dyn Error>> {
    let (_directory, database, clock) = open_database(1_000).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(&database, user, serde_json::json!({"name":"Alex"})).await?;
    let original = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-01",
        Some("12:00"),
        "60 kg",
    )
    .await?;
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(60))
    );
    let original_state = log_state(&database, original.id).await?;
    let log_count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profile_logs WHERE profile_id = ?")
            .bind(profile.id.get())
            .fetch_one(&database.pool)
            .await?;

    sqlx::query(
        "CREATE TRIGGER reject_profile_cache_update
         BEFORE UPDATE OF data_json ON profiles
         BEGIN SELECT RAISE(ABORT, 'blocked cache update'); END",
    )
    .execute(&database.pool)
    .await?;

    clock.store(2_000, Ordering::SeqCst);
    let failed_create = create_weight_log(
        &database,
        user,
        profile.id,
        "2024-01-02",
        Some("12:00"),
        "70 kg",
    )
    .await;
    assert!(failed_create.is_err());
    let log_count_after_create: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profile_logs WHERE profile_id = ?")
            .bind(profile.id.get())
            .fetch_one(&database.pool)
            .await?;
    assert_eq!(log_count_after_create, log_count_before);
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(60))
    );

    let failing_patch: PatchLog = serde_json::from_str(
        r#"{"day":"2025-01-01","entry":{"type":"biometric","measurement":{"type":"weight","value":"80 kg"}}}"#,
    )?;
    assert!(
        database
            .patch_profile_log(user, profile.id, original.id, &failing_patch)
            .await
            .is_err()
    );
    assert_eq!(
        log_state(&database, original.id).await?.day,
        original_state.day
    );
    assert_eq!(
        serde_json::from_str::<ProfileLogData>(
            &log_state(&database, original.id).await?.data_json
        )?,
        original.data
    );
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(60))
    );

    assert!(
        database
            .delete_profile_log(user, profile.id, original.id)
            .await
            .is_err()
    );
    assert_eq!(
        log_state(&database, original.id).await?.day,
        original_state.day
    );
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(60))
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_weight_creates_leave_cache_at_latest_local_day() -> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database(1_000).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(&database, user, serde_json::json!({"name":"Alex"})).await?;
    let database = Arc::new(database);
    let barrier = Arc::new(Barrier::new(3));
    let first_database = Arc::clone(&database);
    let first_barrier = Arc::clone(&barrier);
    let first = tokio::spawn(async move {
        first_barrier.wait().await;
        create_weight_log(
            &first_database,
            user,
            profile.id,
            "2024-01-01",
            Some("12:00"),
            "70 kg",
        )
        .await
        .map_err(|error| error.to_string())
    });
    let second_database = Arc::clone(&database);
    let second_barrier = Arc::clone(&barrier);
    let second = tokio::spawn(async move {
        second_barrier.wait().await;
        create_weight_log(
            &second_database,
            user,
            profile.id,
            "2024-01-02",
            Some("12:00"),
            "80 kg",
        )
        .await
        .map_err(|error| error.to_string())
    });
    barrier.wait().await;
    first
        .await
        .map_err(std::io::Error::other)?
        .map_err(std::io::Error::other)?;
    second
        .await
        .map_err(std::io::Error::other)?
        .map_err(std::io::Error::other)?;
    assert_eq!(
        current_weight(&get_profile(&database, user, profile.id).await?),
        Some(Decimal::from(80))
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM profile_logs WHERE profile_id = ?")
        .bind(profile.id.get())
        .fetch_one(&database.pool)
        .await?;
    assert_eq!(count, 2);
    drop(database);
    Ok(())
}

#[tokio::test]
async fn migration_has_generated_projection_columns_indexes_and_profile_restrict()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database(1_000).await?;
    let user = insert_user(&database, 1).await?;
    let profile = create_profile(&database, user, serde_json::json!({"name":"Alex"})).await?;
    let strict: i64 =
        sqlx::query_scalar("SELECT strict FROM pragma_table_list WHERE name = 'profile_logs'")
            .fetch_one(&database.pool)
            .await?;
    assert_eq!(strict, 1);

    sqlx::query(
        "INSERT INTO profile_logs (
             profile_id, day, time, data_json, created_at, updated_at
         ) VALUES (?, '2024-01-01', '12:00:00.000', '{\"type\":\"food\",\"food_id\":42}', 1, NULL)",
    )
    .bind(profile.id.get())
    .execute(&database.pool)
    .await?;
    let projections: (String, Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT entry_type, subtype, source_id FROM profile_logs WHERE profile_id = ?",
    )
    .bind(profile.id.get())
    .fetch_one(&database.pool)
    .await?;
    assert_eq!(projections, ("food".to_owned(), None, Some(42)));

    for index_name in [
        "profile_logs_profile_id_id",
        "profile_logs_profile_day_id",
        "profile_logs_latest_weight",
    ] {
        let found: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name = ?",
        )
        .bind(index_name)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(found, 1, "missing index {index_name}");
    }
    assert!(
        sqlx::query("DELETE FROM profiles WHERE id = ?")
            .bind(profile.id.get())
            .execute(&database.pool)
            .await
            .is_err()
    );
    database.close().await;
    Ok(())
}
