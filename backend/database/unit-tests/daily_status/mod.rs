use crate::daily_status::{DailyStatusQuery, DailyStatusStore};
use crate::identity::UserId;
use crate::profiles::{CreateProfile, ProfileId, Profiles};
use crate::runtime::Database;
use crate::tests::{insert_user, open_database_with_clock};
use serde_json::{Value, json};
use std::error::Error;
use std::sync::atomic::Ordering;

async fn profile(
    db: &Database,
    user: UserId,
    settings: Value,
) -> Result<ProfileId, Box<dyn Error>> {
    let input: CreateProfile = serde_json::from_value(json!({"name":"Alex", "settings":settings}))?;
    Ok(db.create_profile(user, &input).await?.id)
}
async fn add(
    db: &Database,
    id: ProfileId,
    day: &str,
    time: &str,
    data: Value,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO profile_logs (profile_id, day, time, data_json, created_at) VALUES (?, ?, ?, ?, 1)")
        .bind(id.get()).bind(day).bind(time).bind(data.to_string()).execute(&db.pool).await?;
    Ok(())
}
async fn report(
    db: &Database,
    user: UserId,
    id: ProfileId,
    query: Value,
) -> Result<Value, Box<dyn Error>> {
    let query: DailyStatusQuery = serde_json::from_value(query)?;
    Ok(serde_json::to_value(
        db.daily_status(user, id, &query)
            .await?
            .ok_or_else(|| std::io::Error::other("profile not found"))?,
    )?)
}

#[tokio::test]
async fn dates_timezone_empty_days_and_ownership() -> Result<(), Box<dyn Error>> {
    let now = chrono::DateTime::parse_from_rfc3339("2024-01-02T07:59:59Z")?.timestamp_millis();
    let (_dir, db, clock) = open_database_with_clock(u64::try_from(now)?).await?;
    let user = insert_user(&db, 1).await?;
    let other = insert_user(&db, 2).await?;
    let id = profile(&db, user, json!({"timezone":"America/Los_Angeles"})).await?;
    let first = report(&db, user, id, json!({})).await?;
    assert_eq!(first["start"], "2024-01-01");
    assert_eq!(first["days"][0]["calories_consumed"], "0 kcal");
    assert!(first["days"][0]["steps"].is_null());
    assert!(first["days"][0]["expenditure"]["total"].is_null());
    clock.store(u64::try_from(now + 1_000)?, Ordering::SeqCst);
    assert_eq!(
        report(&db, user, id, json!({})).await?["start"],
        "2024-01-02"
    );
    for query in [json!({"start":"2024-01-03"}), json!({"stop":"2024-01-03"})] {
        let result = report(&db, user, id, query).await?;
        assert_eq!(result["start"], "2024-01-03");
        assert_eq!(result["stop"], "2024-01-03");
        assert_eq!(result["days"].as_array().map(Vec::len), Some(1));
    }
    for (day, calories) in [
        ("2023-12-31", "99 kcal"),
        ("2024-01-01", "1 kcal"),
        ("2024-01-03", "3 kcal"),
        ("2024-01-04", "99 kcal"),
    ] {
        add(
            &db,
            id,
            day,
            "12:00:00.000",
            json!({"type":"exercise", "calories_burned":calories}),
        )
        .await?;
    }
    let result = report(
        &db,
        user,
        id,
        json!({"start":"2024-01-01", "stop":"2024-01-03"}),
    )
    .await?;
    assert_eq!(result["days"].as_array().map(Vec::len), Some(3));
    assert_eq!(result["days"][2]["day"], "2024-01-03");
    assert_eq!(result["days"][0]["expenditure"]["exercise"], "1 kcal");
    assert_eq!(result["days"][1]["expenditure"]["exercise"], "0 kcal");
    assert_eq!(result["days"][2]["expenditure"]["exercise"], "3 kcal");
    assert!(result["days"][0]["goals"]["calories"].is_null());
    let reverse: DailyStatusQuery =
        serde_json::from_value(json!({"start":"2024-01-03", "stop":"2024-01-01"}))?;
    assert!(db.daily_status(user, id, &reverse).await.is_err());
    assert!(serde_json::from_value::<DailyStatusQuery>(json!({"start":"2024-02-30"})).is_err());
    assert!(
        db.daily_status(other, id, &DailyStatusQuery::default())
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn aggregates_food_exercise_and_latest_activity_with_goals() -> Result<(), Box<dyn Error>> {
    let (_dir, db, _) = open_database_with_clock(1_000).await?;
    let user = insert_user(&db, 1).await?;
    let id = profile(
        &db,
        user,
        json!({"daily_water_goal":"1 l", "daily_step_goal":1000,
        "daily_active_time_goal":"30 min", "weight_change_goal":"0 kg/week",
        "bmr":{"mode":"custom", "value":"1000 kcal/day"},
        "baseline_activity":{"mode":"custom", "value":"200 kcal/day"}}),
    )
    .await?;
    let day = "2024-01-01";
    let nutrition = json!({"calories":"~600 kcal", "fat":{"total":"1 g", "saturated":"1 mg", "monounsaturated":"1 mg", "polyunsaturated":"1 mg", "trans":"1 mg"},
        "carbohydrates":{"including_fibre":"1 g", "excluding_fibre":"1 g", "sugars":{"total":"1 g", "added":"1 g", "lactose":"1 g"}, "starch":"1 g"},
        "fibre":"1 g", "protein":"1 g", "sodium":"1 mg", "salt":"1 g", "cholesterol":"1 mg", "water":"500 g", "caffeine":"~1 mg", "choline":"1 mg",
        "minerals":{"calcium":"1 mg", "iron":"1 mg", "magnesium":"1 mg", "phosphorus":"1 mg", "potassium":"1 mg", "zinc":"1 mg", "copper":"1 mg", "manganese":"1 mg", "selenium":"1 mg"},
        "vitamins":{"vitamin_a":"1 µg", "vitamin_c":"1 mg", "vitamin_d":"1 µg", "vitamin_e":"1 mg", "vitamin_k1":"1 µg", "vitamin_b1":"1 mg", "vitamin_b2":"1 mg", "vitamin_b3":"1 mg", "vitamin_b5":"1 mg", "vitamin_b6":"1 mg", "vitamin_b9":"1 µg", "vitamin_b12":"1 µg"}});
    add(
        &db,
        id,
        day,
        "12:00:00.000",
        json!({"type":"food", "nutrition":nutrition}),
    )
    .await?;
    add(&db, id, day, "13:00:00.000", json!({"type":"food", "nutrition":{"calories":"700 kcal", "protein":"500 mg", "water":"750 g"}})).await?;
    for calories in ["50 kcal", "75 kcal"] {
        add(
            &db,
            id,
            day,
            "14:00:00.000",
            json!({"type":"exercise", "calories_burned":calories}),
        )
        .await?;
    }
    for (time, kind, value) in [
        ("08:00:00.000", "steps", json!(9000)),
        ("09:00:00.000", "steps", json!(1500)),
        ("09:00:00.000", "steps", json!(1200)),
        ("10:00:00.000", "active_time", json!("45 min")),
    ] {
        add(
            &db,
            id,
            day,
            time,
            json!({"type":"daily_activity", "reading":{"type":kind, "value":value}}),
        )
        .await?;
    }
    let result = report(&db, user, id, json!({"start":day, "stop":"2024-01-02"})).await?;
    let status = &result["days"][0];
    assert_eq!(status["calories_consumed"], "1300 kcal");
    assert_eq!(status["nutrients"]["protein"], "1.50 g");
    assert_eq!(status["nutrients"]["minerals"]["calcium"], "0.001 g");
    for (group, names, total) in [
        (
            "fat",
            "saturated monounsaturated polyunsaturated trans",
            "0.001 g",
        ),
        (
            "carbohydrates",
            "including_fibre excluding_fibre starch",
            "1 g",
        ),
        (
            "minerals",
            "calcium iron magnesium phosphorus potassium zinc copper manganese selenium",
            "0.001 g",
        ),
        (
            "vitamins",
            "vitamin_c vitamin_e vitamin_b1 vitamin_b2 vitamin_b3 vitamin_b5 vitamin_b6",
            "0.001 g",
        ),
        (
            "vitamins",
            "vitamin_a vitamin_d vitamin_k1 vitamin_b9 vitamin_b12",
            "0.000001 g",
        ),
    ] {
        for name in names.split_whitespace() {
            assert_eq!(status["nutrients"][group][name], total, "{group}.{name}");
        }
    }
    for name in "total added lactose".split_whitespace() {
        assert_eq!(
            status["nutrients"]["carbohydrates"]["sugars"][name], "1 g",
            "sugars.{name}"
        );
    }
    assert_eq!(status["nutrients"]["fat"]["total"], "1 g");
    for name in "fibre salt".split_whitespace() {
        assert_eq!(status["nutrients"][name], "1 g", "{name}");
    }
    for name in "sodium cholesterol choline".split_whitespace() {
        assert_eq!(status["nutrients"][name], "0.001 g", "{name}");
    }
    assert_eq!(status["nutrients"]["caffeine"], "0.001 g");
    assert_eq!(status["nutrients"]["water"], "1250 g");
    assert_eq!(status["water"], "1250 ml");
    assert_eq!(status["steps"], 1200);
    assert_eq!(status["active_time"], "2700 s");
    assert_eq!(
        status["expenditure"],
        json!({"maintenance":"1200 kcal", "exercise":"125 kcal", "total":"1325 kcal"})
    );
    assert_eq!(status["goals"]["calories"]["target"], "1200 kcal");
    let calorie_percent: rust_decimal::Decimal = status["goals"]["calories"]["percent"]
        .as_str()
        .ok_or("missing percentage")?
        .parse()?;
    assert_eq!(
        calorie_percent,
        rust_decimal::Decimal::from(1300) / rust_decimal::Decimal::from(1200)
            * rust_decimal::Decimal::from(100)
    );
    assert!(result["days"][1]["goals"]["steps"]["percent"].is_null());
    assert!(result["days"][1]["goals"]["active_time"]["percent"].is_null());
    assert_eq!(status["goals"]["water"]["percent"], "125");
    assert_eq!(status["goals"]["steps"]["percent"], "120");
    assert_eq!(status["goals"]["active_time"]["percent"], "150");
    assert_eq!(result["days"][1]["expenditure"]["maintenance"], "1200 kcal");
    Ok(())
}

#[tokio::test]
async fn no_pagination_limit_and_zero_goals() -> Result<(), Box<dyn Error>> {
    let (_dir, db, _) = open_database_with_clock(1_000).await?;
    let user = insert_user(&db, 1).await?;
    let id = profile(
        &db,
        user,
        json!({"daily_water_goal":"0 l", "daily_step_goal":0, "daily_active_time_goal":"0 s", "weight_change_goal":"-1 kg/week", "bmr":{"mode":"custom", "value":"1000 kcal/day"}, "baseline_activity":{"mode":"custom", "value":"0 kcal/day"}}),
    )
    .await?;
    for _ in 0..101 {
        add(
            &db,
            id,
            "2024-01-01",
            "12:00:00.000",
            json!({"type":"food", "nutrition":{"calories":"1 kcal"}}),
        )
        .await?;
    }
    let result = report(&db, user, id, json!({"start":"2024-01-01"})).await?;
    let day = &result["days"][0];
    assert_eq!(day["calories_consumed"], "101 kcal");
    for goal in ["calories", "water", "steps", "active_time"] {
        assert!(day["goals"][goal]["percent"].is_null());
    }
    Ok(())
}

#[test]
fn minimal_nutrition_profile_retains_optional_nutrient_defaults() -> Result<(), Box<dyn Error>> {
    let profile: crate::nutrition_model::NutritionProfile =
        serde_json::from_value(json!({"per":"100 g", "energy":"100 kcal"}))?;
    assert!(profile.nutrients.protein.is_none());
    assert!(profile.nutrients.fat.total.is_none());
    assert!(profile.nutrients.minerals.calcium.is_none());
    assert!(profile.nutrients.vitamins.vitamin_c.is_none());
    Ok(())
}
