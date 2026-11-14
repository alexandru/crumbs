use std::error::Error;
use std::sync::atomic::Ordering;

use sqlx::FromRow;

use crate::clock::UnixMillis;
use crate::nutrition_model::Measurement;
use crate::profiles::*;
use crate::runtime::Database;
use crate::tests::{insert_user, open_database_with_clock};

fn create_input(value: &str) -> Result<CreateProfile, Box<dyn Error>> {
    Ok(serde_json::from_str(value)?)
}

#[derive(FromRow)]
struct Timestamps {
    created_at: UnixMillis,
    updated_at: Option<UnixMillis>,
}

async fn timestamps(database: &Database, id: ProfileId) -> Result<Timestamps, sqlx::Error> {
    sqlx::query_as::<_, Timestamps>("SELECT created_at, updated_at FROM profiles WHERE id = ?")
        .bind(id.get())
        .fetch_one(&database.pool)
        .await
}

#[tokio::test]
async fn profile_names_are_unique_per_account_and_listing_is_scoped_and_cursor_based()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database_with_clock(1_000).await?;
    let alice = insert_user(&database, 1).await?;
    let bob = insert_user(&database, 2).await?;

    let first = database
        .create_profile(alice, &create_input(r#"{"name":" Alex "}"#)?)
        .await?;
    assert_eq!(first.name.as_str(), "Alex");

    let duplicate = database
        .create_profile(alice, &create_input(r#"{"name":"aLEX"}"#)?)
        .await;
    assert!(matches!(duplicate, Err(ProfileError::DuplicateName)));

    let second = database
        .create_profile(alice, &create_input(r#"{"name":"Cristi"}"#)?)
        .await?;
    let bob_profile = database
        .create_profile(bob, &create_input(r#"{"name":"alex"}"#)?)
        .await?;
    assert_eq!(database.profile(bob, first.id).await?, None);
    assert_eq!(database.profile(alice, bob_profile.id).await?, None);

    let page = database
        .list_profiles(
            alice,
            &ProfileListQuery {
                last_id: None,
                limit: Some(ProfileLimit::new(1)?),
            },
        )
        .await?;
    assert_eq!(
        page.items
            .iter()
            .map(|profile| profile.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
    assert_eq!(page.last_id, Some(first.id));
    let next = database
        .list_profiles(
            alice,
            &ProfileListQuery {
                last_id: page.last_id,
                limit: Some(ProfileLimit::new(1)?),
            },
        )
        .await?;
    assert_eq!(
        next.items
            .iter()
            .map(|profile| profile.id)
            .collect::<Vec<_>>(),
        vec![second.id]
    );
    assert_eq!(next.last_id, Some(second.id));
    assert_eq!(
        database
            .list_profiles(bob, &ProfileListQuery::default())
            .await?
            .items
            .len(),
        1
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn create_applies_defaults_and_patch_preserves_cache_and_timestamp_semantics()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, clock) = open_database_with_clock(1_000).await?;
    let user = insert_user(&database, 1).await?;

    let created = database
        .create_profile(user, &create_input(r#"{"name":"Alex"}"#)?)
        .await?;
    assert_eq!(created.data.settings.timezone.as_str(), "UTC");
    assert_eq!(created.data.settings.bmr, BmrSetting::Calculated);
    assert_eq!(created.data.settings.baseline_activity, None);
    assert_eq!(created.data.cache.current_weight, None);
    assert_eq!(created.derived.bmr(), None);
    assert_eq!(created.derived.baseline_activity(), None);
    assert_eq!(created.derived.maintenance_calories(), None);
    assert_eq!(timestamps(&database, created.id).await?.created_at, 1_000);
    assert_eq!(timestamps(&database, created.id).await?.updated_at, None);

    sqlx::query(
        "UPDATE profiles
         SET data_json = json_set(data_json, '$.cache.current_weight', '70 kg')
         WHERE id = ?",
    )
    .bind(created.id.get())
    .execute(&database.pool)
    .await?;

    clock.store(2_000, Ordering::SeqCst);
    let case_only_rename: PatchProfile = serde_json::from_str(r#"{"name":"ALEX"}"#)?;
    let changed = database
        .patch_profile(user, created.id, &case_only_rename)
        .await?
        .ok_or_else(|| std::io::Error::other("created profile was not found"))?;
    assert_eq!(changed.name.as_str(), "ALEX");
    assert_eq!(
        changed
            .data
            .cache
            .current_weight
            .map(|weight| weight.value().to_string()),
        Some("70 kg".to_owned())
    );
    assert_eq!(timestamps(&database, created.id).await?.created_at, 1_000);
    assert_eq!(
        timestamps(&database, created.id).await?.updated_at,
        Some(2_000)
    );

    clock.store(3_000, Ordering::SeqCst);
    let no_op: PatchProfile = serde_json::from_str("{}")?;
    let unchanged = database
        .patch_profile(user, created.id, &no_op)
        .await?
        .ok_or_else(|| std::io::Error::other("created profile was not found"))?;
    assert_eq!(unchanged.name.as_str(), "ALEX");
    assert_eq!(
        timestamps(&database, created.id).await?.updated_at,
        Some(2_000)
    );

    assert!(
        serde_json::from_str::<PatchProfile>(r#"{"cache":{"current_weight":"1 kg"}}"#).is_err()
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn patch_nulls_and_mode_replacement_persist_independently() -> Result<(), Box<dyn Error>> {
    let (_directory, database, clock) = open_database_with_clock(10_000).await?;
    let user = insert_user(&database, 1).await?;

    let created = database
        .create_profile(
            user,
            &create_input(
                r#"{"name":"Amalia","settings":{"birth_date":"1982-09","weight_goal":"70 kg","baseline_activity":{"mode":"custom","value":"0 kcal/day"}}}"#,
            )?,
        )
        .await?;
    assert_eq!(
        created.data.settings.birth_date,
        Some(BirthDate::year_month(1982, 9)?)
    );
    assert_eq!(
        created
            .data
            .settings
            .weight_goal
            .map(|weight| weight.value().to_string()),
        Some("70 kg".to_owned())
    );
    assert_eq!(
        created.data.settings.baseline_activity,
        Some(BaselineActivity::Custom {
            value: "0 kcal/day".parse()?,
        })
    );

    clock.store(11_000, Ordering::SeqCst);
    let custom_bmr = serde_json::from_str::<PatchProfile>(
        r#"{"bmr":{"mode":"custom","value":"1200 kcal/day"},"weight_goal":null}"#,
    )?;
    let updated = database
        .patch_profile(user, created.id, &custom_bmr)
        .await?
        .ok_or_else(|| std::io::Error::other("created profile was not found"))?;
    assert!(matches!(
        updated.data.settings.bmr,
        BmrSetting::Custom { .. }
    ));
    assert_eq!(updated.data.settings.weight_goal, None);
    assert_eq!(
        updated.derived.bmr().map(|value| value.to_string()),
        Some("1200 kcal/day".to_owned())
    );
    assert_eq!(
        updated
            .derived
            .baseline_activity()
            .map(|value| value.to_string()),
        Some("0 kcal/day".to_owned())
    );
    assert_eq!(
        updated
            .derived
            .maintenance_calories()
            .map(|value| value.to_string()),
        Some("1200 kcal/day".to_owned())
    );

    let reset: PatchProfile = serde_json::from_str(r#"{"bmr":null,"birth_date":null}"#)?;
    let reset_profile = database
        .patch_profile(user, created.id, &reset)
        .await?
        .ok_or_else(|| std::io::Error::other("created profile was not found"))?;
    assert_eq!(reset_profile.data.settings.bmr, BmrSetting::Calculated);
    assert_eq!(reset_profile.data.settings.birth_date, None);
    assert_eq!(reset_profile.derived.bmr(), None);
    assert_eq!(
        reset_profile.derived.baseline_activity(),
        Some("0 kcal/day".parse()?)
    );
    assert_eq!(reset_profile.derived.maintenance_calories(), None);
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_calculation_timestamps_do_not_reserve_profile_names() -> Result<(), Box<dyn Error>>
{
    let (_directory, database, clock) = open_database_with_clock(1_000).await?;
    let user = insert_user(&database, 1).await?;

    let input = create_input(r#"{"name":"Overflow"}"#)?;
    clock.store(u64::try_from(i64::MAX)?, Ordering::SeqCst);
    assert!(matches!(
        database.create_profile(user, &input).await,
        Err(ProfileError::Calculation(_))
    ));
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM profiles WHERE user_id = ?")
        .bind(user.get())
        .fetch_one(&database.pool)
        .await?;
    assert_eq!(rows, 0);
    clock.store(1_000, Ordering::SeqCst);
    database.create_profile(user, &input).await?;
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn profile_migration_enforces_strict_json_and_restricts_user_deletion()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database_with_clock(1_000).await?;
    let user = insert_user(&database, 1).await?;

    database
        .create_profile(user, &create_input(r#"{"name":"Alex"}"#)?)
        .await?;

    let strict: i64 =
        sqlx::query_scalar("SELECT strict FROM pragma_table_list WHERE name = 'profiles'")
            .fetch_one(&database.pool)
            .await?;
    assert_eq!(strict, 1);
    let unique_index: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'index' AND name = 'profiles_user_name_key_unique'",
    )
    .fetch_one(&database.pool)
    .await?;
    assert_eq!(unique_index, 1);

    let invalid_json = sqlx::query(
        "INSERT INTO profiles (user_id, name, name_key, data_json, created_at)
         VALUES (?, 'Bad', 'bad', '{', 1)",
    )
    .bind(user.get())
    .execute(&database.pool)
    .await;
    assert!(invalid_json.is_err());
    assert!(
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user.get())
            .execute(&database.pool)
            .await
            .is_err()
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn malformed_stored_profile_data_is_reported_as_domain_corruption()
-> Result<(), Box<dyn Error>> {
    let (_directory, database, _) = open_database_with_clock(1_000).await?;
    let user = insert_user(&database, 1).await?;

    let profile = database
        .create_profile(user, &create_input(r#"{"name":"Alex"}"#)?)
        .await?;
    sqlx::query("UPDATE profiles SET data_json = '{}' WHERE id = ?")
        .bind(profile.id.get())
        .execute(&database.pool)
        .await?;
    assert!(matches!(
        database.profile(user, profile.id).await,
        Err(ProfileError::InvalidStoredData)
    ));
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn weight_change_goal_persists_and_recalculates_without_changing_other_values()
-> Result<(), Box<dyn Error>> {
    let (directory, database, clock) = open_database_with_clock(1_000).await?;
    let user = insert_user(&database, 1).await?;

    let created = database.create_profile(user, &create_input(r#"{"name":"Alex","settings":{"weight_change_goal":"-0.5 kg/week","weight_goal":"70 kg","bmr":{"mode":"custom","value":"1200 kcal/day"},"baseline_activity":{"mode":"custom","value":"300 kcal/day"}}}"#)?).await?;
    assert_eq!(
        created.derived.calorie_balance_goal().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(-550))
    );
    assert_eq!(
        created.derived.maintenance_calories().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(1500))
    );
    assert_eq!(created.data.cache.current_weight, None);
    let id = created.id;
    clock.store(2_000, Ordering::SeqCst);
    let patch: PatchProfile = serde_json::from_str(r#"{"weight_change_goal":"-0.25 kg/week"}"#)?;
    let updated = database
        .patch_profile(user, id, &patch)
        .await?
        .ok_or("missing profile")?;
    assert_eq!(
        updated.derived.calorie_balance_goal().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(-275))
    );
    assert_eq!(
        updated.derived.maintenance_calories().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(1500))
    );
    assert_eq!(
        updated.data.settings.weight_goal,
        created.data.settings.weight_goal
    );
    assert_eq!(updated.data.cache, created.data.cache);
    clock.store(3_000, Ordering::SeqCst);
    let noop: PatchProfile = serde_json::from_str("{}")?;
    let unchanged = database
        .patch_profile(user, id, &noop)
        .await?
        .ok_or("missing profile")?;
    assert_eq!(
        unchanged.data.settings.weight_change_goal,
        updated.data.settings.weight_change_goal
    );
    assert_eq!(timestamps(&database, id).await?.updated_at, Some(2_000));
    database.close().await;
    drop(database);
    let reopened = Database::open(crate::DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
    let stored = reopened.profile(user, id).await?.ok_or("missing profile")?;
    assert_eq!(
        stored.data.settings.weight_change_goal,
        updated.data.settings.weight_change_goal
    );
    assert_eq!(
        stored.derived.calorie_balance_goal().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(-275))
    );
    let gain: PatchProfile = serde_json::from_str(r#"{"weight_change_goal":"0.5 kg/week"}"#)?;
    let gained = reopened
        .patch_profile(user, id, &gain)
        .await?
        .ok_or("missing profile")?;
    assert_eq!(
        gained.derived.calorie_balance_goal().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(550))
    );
    assert_eq!(
        gained.derived.maintenance_calories().map(|v| v.value()),
        Some(rust_decimal::Decimal::from(1500))
    );
    assert_eq!(
        gained.data.settings.weight_goal,
        created.data.settings.weight_goal
    );
    assert_eq!(gained.data.cache, created.data.cache);
    let clear: PatchProfile = serde_json::from_str(r#"{"weight_change_goal":null}"#)?;
    let cleared = reopened
        .patch_profile(user, id, &clear)
        .await?
        .ok_or("missing profile")?;
    assert_eq!(cleared.data.settings.weight_change_goal, None);
    assert_eq!(cleared.derived.calorie_balance_goal(), None);
    assert_eq!(
        cleared.data.settings.weight_goal,
        created.data.settings.weight_goal
    );
    reopened.close().await;
    Ok(())
}
