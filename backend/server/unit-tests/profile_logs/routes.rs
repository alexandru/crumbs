use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use crumbs_database::identity::{UserId, Username, Users};
use crumbs_database::profile_logs::{CreateLog, ProfileLogs};
use crumbs_database::profiles::ProfileId;
use serde_json::{Value, json};

use crate::oauth::tests::{TEST_REST_ACCESS_TOKEN, TEST_REST_WRITE_ACCESS_TOKEN};
use crate::tests::{
    TestApplication, http_json_request, http_method_request, http_request, http_request_with_token,
};

use super::{
    assert_calories, assert_mass, create_test_food, create_test_profile, seed_other_access_token,
    seed_profile_owner,
};

const OTHER_REST_TOKEN: &str = "test-other-profile-log-rest-token";
const OTHER_REST_REFRESH: &str = "test-other-profile-log-rest-refresh";
const OTHER_REST_CLIENT: &str = "test-other-profile-log-rest-client";

#[tokio::test]
async fn rest_profile_logs_update_current_weight_and_support_date_filters_and_cursors()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let profile_uri = format!("/profiles/{profile_id}");
    let logs_uri = format!("{profile_uri}/logs");

    let first = create_log(&app, &logs_uri, None, "2025-06-01", "80 kg").await?;
    assert_eq!(first.status, StatusCode::CREATED, "{:?}", first.json);
    let first_id = first.json["id"].as_i64().ok_or("created log has no ID")?;
    assert_eq!(first.json["profile_id"], profile_id);
    assert_eq!(first.json["day"], "2025-06-01");
    assert!(first.json.get("created_at").is_none());
    assert!(first.json.get("updated_at").is_none());
    assert_eq!(
        first.json["data"],
        json!({"type":"biometric", "measurement":{"type":"weight", "value":"80 kg"}})
    );
    let time = first.json["time"].as_str().ok_or("log time is missing")?;
    assert_eq!(time.len(), 12);
    assert_local_time_near_now(time, 5 * 60 * 60 + 30 * 60)?;

    let first_uri = format!("{logs_uri}/{first_id}");
    let null_time = app
        .request(http_json_request(
            "PATCH",
            &first_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"time":null}),
        )?)
        .await?;
    assert_eq!(null_time.status, StatusCode::BAD_REQUEST);
    assert_eq!(null_time.json["code"], "invalid_input");
    let zero_weight = app
        .request(http_json_request(
            "POST",
            &logs_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            log_input("2025-06-01", None, "0 kg"),
        )?)
        .await?;
    assert_eq!(zero_weight.status, StatusCode::BAD_REQUEST);
    assert_eq!(zero_weight.json["code"], "invalid_input");
    let extra_entry_field = app
        .request(http_json_request(
            "POST",
            &logs_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({
                "day": "2025-06-01",
                "entry": {
                    "type": "biometric",
                    "measurement": {"type": "weight", "value": "80 kg"},
                    "extra": true
                }
            }),
        )?)
        .await?;
    assert_eq!(extra_entry_field.status, StatusCode::BAD_REQUEST);
    assert_eq!(extra_entry_field.json["code"], "invalid_input");

    let after_first = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(after_first.json["data"]["cache"]["current_weight"], "80 kg");
    let first_bmr = after_first.json["derived"]["bmr"].clone();
    assert!(first_bmr.is_string());

    let second = create_log(&app, &logs_uri, Some("08:00:00.125"), "2025-06-02", "90 kg").await?;
    assert_eq!(second.status, StatusCode::CREATED, "{:?}", second.json);
    let second_id = second.json["id"].as_i64().ok_or("created log has no ID")?;
    assert_eq!(second.json["time"], "08:00:00.125");

    let after_second = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        after_second.json["data"]["cache"]["current_weight"],
        "90 kg"
    );
    assert_ne!(after_second.json["derived"]["bmr"], first_bmr);
    assert!(after_second.json["derived"]["maintenance_calories"].is_string());

    let date_filter = app
        .request(http_request(&format!(
            "{logs_uri}?from=2025-06-02&to=2025-06-02&entry_type=biometric&subtype=weight"
        ))?)
        .await?;
    assert_eq!(date_filter.status, StatusCode::OK, "{:?}", date_filter.json);
    assert_eq!(date_filter.json["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(date_filter.json["items"][0]["id"], second_id);
    assert_eq!(date_filter.json["items"][0]["day"], "2025-06-02");

    let invalid_range = app
        .request(http_request(&format!(
            "{logs_uri}?from=2025-06-03&to=2025-06-02"
        ))?)
        .await?;
    assert_eq!(invalid_range.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_range.json["code"], "invalid_input");
    let invalid_subtype = app
        .request(http_request(&format!("{logs_uri}?subtype=weight"))?)
        .await?;
    assert_eq!(invalid_subtype.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_subtype.json["code"], "invalid_input");

    let second_uri = format!("{logs_uri}/{second_id}");
    let fetched = app.request(http_request(&second_uri)?).await?;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.json, second.json);

    let time_patch = app
        .request(http_json_request(
            "PATCH",
            &second_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"time":"09:00"}),
        )?)
        .await?;
    assert_eq!(time_patch.status, StatusCode::OK, "{:?}", time_patch.json);
    assert_eq!(time_patch.json["time"], "09:00:00.000");
    assert_eq!(time_patch.json["data"], second.json["data"]);

    let moved_earlier = app
        .request(http_json_request(
            "PATCH",
            &second_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({
                "day":"2025-05-31",
                "entry":{"type":"biometric", "measurement":{"type":"weight", "value":"85 kg"}}
            }),
        )?)
        .await?;
    assert_eq!(
        moved_earlier.status,
        StatusCode::OK,
        "{:?}",
        moved_earlier.json
    );
    assert_eq!(moved_earlier.json["data"]["measurement"]["value"], "85 kg");
    let after_earlier_patch = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        after_earlier_patch.json["data"]["cache"]["current_weight"],
        "80 kg"
    );
    assert_eq!(after_earlier_patch.json["derived"]["bmr"], first_bmr);

    let moved_later = app
        .request(http_json_request(
            "PATCH",
            &second_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"day":"2025-06-02"}),
        )?)
        .await?;
    assert_eq!(moved_later.status, StatusCode::OK, "{:?}", moved_later.json);
    assert_eq!(moved_later.json["data"]["measurement"]["value"], "85 kg");
    let after_later_patch = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        after_later_patch.json["data"]["cache"]["current_weight"],
        "85 kg"
    );
    assert_ne!(after_later_patch.json["derived"]["bmr"], first_bmr);

    let deleted_second = app
        .request(http_method_request(
            "DELETE",
            &second_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(deleted_second.status, StatusCode::NO_CONTENT);
    let after_delete_second = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        after_delete_second.json["data"]["cache"]["current_weight"],
        "80 kg"
    );

    let deleted_first = app
        .request(http_method_request(
            "DELETE",
            &first_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(deleted_first.status, StatusCode::NO_CONTENT);
    let after_delete_first = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        after_delete_first.json["data"]["cache"]["current_weight"],
        Value::Null
    );
    assert_eq!(after_delete_first.json["derived"]["bmr"], Value::Null);
    assert_eq!(
        after_delete_first.json["derived"]["maintenance_calories"],
        Value::Null
    );

    seed_logs_for_pagination(&app, profile_id, 101).await?;
    let first_page = app.request(http_request(&logs_uri)?).await?;
    assert_eq!(first_page.status, StatusCode::OK, "{:?}", first_page.json);
    let items = first_page.json["items"]
        .as_array()
        .ok_or("first page has no items")?;
    assert_eq!(items.len(), 100);
    let ids = items
        .iter()
        .map(|item| item["id"].as_i64().ok_or("log ID is missing"))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        ids.iter()
            .zip(ids.iter().skip(1))
            .all(|(first, second)| first > second)
    );
    let cursor = first_page.json["last_id"]
        .as_i64()
        .ok_or("first page has no cursor")?;
    assert_eq!(cursor, *ids.last().ok_or("first page is empty")?);

    let second_page = app
        .request(http_request(&format!("{logs_uri}?last_id={cursor}"))?)
        .await?;
    assert_eq!(second_page.status, StatusCode::OK, "{:?}", second_page.json);
    assert_eq!(second_page.json["items"].as_array().map(Vec::len), Some(1));
    assert!(
        second_page.json["items"][0]["id"]
            .as_i64()
            .is_some_and(|id| id < cursor)
    );
    Ok(())
}

#[tokio::test]
async fn rest_profile_logs_resolve_food_inputs_and_list_mixed_entries() -> Result<(), Box<dyn Error>>
{
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let profile_uri = format!("/profiles/{profile_id}");
    let logs_uri = format!("{profile_uri}/logs");
    let mass_food_id = create_test_food(
        &app,
        "Oat bar",
        json!({
            "nutrition": {"per":"100 g", "energy":"180 kcal", "protein":"10 g", "fat":{"total":"8 g"}, "water":"30 g", "minerals":{"magnesium":"20 mg"}, "vitamins":{"vitamin_a":"10 µg"}},
            "serving_sizes": [{"name":"one bar", "quantity":"50 g"}]
        }),
    )
    .await?;
    let volume_food_id = create_test_food(
        &app,
        "Fruit drink",
        json!({
            "nutrition": {"per":"100 ml", "energy":"40 kcal", "protein":"3 g", "sodium":"10 mg"},
            "serving_sizes": [{"name":"one cup", "quantity":"250 ml"}]
        }),
    )
    .await?;

    let weight = create_log(&app, &logs_uri, Some("08:00"), "2025-07-01", "80 kg").await?;
    assert_eq!(weight.status, StatusCode::CREATED, "{:?}", weight.json);

    let named_mass = create_food_log(
        &app,
        &logs_uri,
        "2025-07-01",
        json!({"type":"food", "food_id":mass_food_id, "amount":"2", "serving_size":" one bar "}),
    )
    .await?;
    assert_eq!(
        named_mass.status,
        StatusCode::CREATED,
        "{:?}",
        named_mass.json
    );
    assert_eq!(named_mass.json["data"]["type"], "food");
    assert_eq!(named_mass.json["data"]["food_id"], mass_food_id);
    assert_eq!(named_mass.json["data"]["quantity"], "100 g");
    assert_calories(
        &named_mass.json["data"]["nutrition"]["calories"],
        "180 kcal",
    )?;
    assert_mass(&named_mass.json["data"]["nutrition"]["protein"], "10 g")?;
    assert_mass(&named_mass.json["data"]["nutrition"]["fat"]["total"], "8 g")?;
    assert_eq!(named_mass.json["data"]["description"], "Oat bar");
    assert_eq!(named_mass.json["data"]["input"]["amount"], "2");
    assert_eq!(
        named_mass.json["data"]["input"]["serving_size"]["name"],
        "one bar"
    );
    assert_eq!(
        named_mass.json["data"]["input"]["serving_size"]["quantity"],
        "50 g"
    );

    let grams = create_food_log(
        &app,
        &logs_uri,
        "2025-07-02",
        json!({"type":"food", "food_id":mass_food_id, "amount":"250", "serving_size":"g"}),
    )
    .await?;
    assert_eq!(grams.status, StatusCode::CREATED, "{:?}", grams.json);
    assert_eq!(grams.json["data"]["quantity"], "250 g");
    assert_calories(&grams.json["data"]["nutrition"]["calories"], "450 kcal")?;
    assert_mass(&grams.json["data"]["nutrition"]["protein"], "25 g")?;
    assert_mass(&grams.json["data"]["nutrition"]["water"], "75 g")?;
    assert_mass(
        &grams.json["data"]["nutrition"]["minerals"]["magnesium"],
        "50 mg",
    )?;
    assert_mass(
        &grams.json["data"]["nutrition"]["vitamins"]["vitamin_a"],
        "25 µg",
    )?;
    assert!(grams.json["data"]["nutrition"]["vitamins"]["vitamin_b12"].is_null());
    assert_mass(&grams.json["data"]["nutrition"]["fat"]["total"], "20 g")?;
    assert_eq!(grams.json["data"]["input"]["amount"], "250");
    assert_eq!(grams.json["data"]["input"]["serving_size"]["name"], "g");
    assert_eq!(
        grams.json["data"]["input"]["serving_size"]["quantity"],
        "1 g"
    );

    let millilitres = create_food_log(
        &app,
        &logs_uri,
        "2025-07-03",
        json!({"type":"food", "food_id":volume_food_id, "amount":"300", "serving_size":"ml"}),
    )
    .await?;
    assert_eq!(
        millilitres.status,
        StatusCode::CREATED,
        "{:?}",
        millilitres.json
    );
    assert_eq!(millilitres.json["data"]["quantity"], "300 ml");
    assert_calories(
        &millilitres.json["data"]["nutrition"]["calories"],
        "120 kcal",
    )?;
    assert_mass(&millilitres.json["data"]["nutrition"]["protein"], "9 g")?;
    assert_mass(&millilitres.json["data"]["nutrition"]["sodium"], "30 mg")?;

    let named_volume = create_food_log(
        &app,
        &logs_uri,
        "2025-07-03",
        json!({"type":"food", "food_id":volume_food_id, "amount":"2", "serving_size":"one cup"}),
    )
    .await?;
    assert_eq!(
        named_volume.status,
        StatusCode::CREATED,
        "{:?}",
        named_volume.json
    );
    assert_eq!(named_volume.json["data"]["quantity"], "500 ml");
    assert_calories(
        &named_volume.json["data"]["nutrition"]["calories"],
        "200 kcal",
    )?;
    assert_mass(&named_volume.json["data"]["nutrition"]["protein"], "15 g")?;

    let profile_after_food = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        profile_after_food.json["data"]["cache"]["current_weight"],
        "80 kg"
    );

    let all = app.request(http_request(&logs_uri)?).await?;
    assert_eq!(all.status, StatusCode::OK);
    let all_items = all.json["items"]
        .as_array()
        .ok_or("mixed log list has no items")?;
    assert_eq!(all_items.len(), 5);
    assert!(all_items.iter().any(|item| item["data"]["type"] == "food"));
    assert!(
        all_items
            .iter()
            .any(|item| item["data"]["type"] == "biometric")
    );
    let all_ids = all_items
        .iter()
        .map(|item| item["id"].as_i64().ok_or("log ID is missing"))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        all_ids
            .iter()
            .zip(all_ids.iter().skip(1))
            .all(|(first, second)| first > second)
    );
    let first_id = all_items
        .first()
        .and_then(|item| item["id"].as_i64())
        .ok_or("mixed list first ID is missing")?;
    let after_first = app
        .request(http_request(&format!("{logs_uri}?last_id={first_id}"))?)
        .await?;
    let after_first_items = after_first.json["items"]
        .as_array()
        .ok_or("cursor log list has no items")?;
    assert_eq!(after_first_items.len(), 4);
    assert!(
        after_first_items
            .iter()
            .all(|item| item["id"].as_i64().is_some_and(|id| id < first_id))
    );

    let foods = app
        .request(http_request(&format!(
            "{logs_uri}?entry_type=food&from=2025-07-01&to=2025-07-03"
        ))?)
        .await?;
    assert_eq!(foods.json["items"].as_array().map(Vec::len), Some(4));
    assert!(
        foods.json["items"]
            .as_array()
            .is_some_and(|items| items.iter().all(|item| item["data"]["type"] == "food"))
    );
    let biometrics = app
        .request(http_request(&format!(
            "{logs_uri}?entry_type=biometric&subtype=weight"
        ))?)
        .await?;
    let biometric_items = biometrics.json["items"]
        .as_array()
        .ok_or("biometric list has no items")?;
    assert_eq!(biometric_items.len(), 1);
    let biometric_type = biometric_items
        .first()
        .and_then(|item| item["data"]["type"].as_str());
    assert_eq!(biometric_type, Some("biometric"));
    Ok(())
}

#[tokio::test]
async fn rest_profile_log_food_snapshots_survive_food_changes_and_deletion()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let logs_uri = format!("/profiles/{profile_id}/logs");
    let food_id = create_test_food(
        &app,
        "Original wings",
        json!({
            "nutrition": {"per":"100 g", "energy":"200 kcal", "protein":"20 g", "minerals":{"iron":"4 mg"}, "vitamins":{"vitamin_c":"5 mg"}},
            "serving_sizes": [{"name":"four wings", "quantity":"200 g"}]
        }),
    )
    .await?;
    let created = create_food_log(
        &app,
        &logs_uri,
        "2025-08-01",
        json!({"type":"food", "food_id":food_id, "amount":"1", "serving_size":"four wings"}),
    )
    .await?;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json);
    let log_id = created.json["id"]
        .as_i64()
        .ok_or("food log ID is missing")?;
    let log_uri = format!("{logs_uri}/{log_id}");
    let original_snapshot = created.json["data"].clone();
    assert_eq!(original_snapshot["description"], "Original wings");
    assert_calories(&original_snapshot["nutrition"]["calories"], "400 kcal")?;
    assert_mass(&original_snapshot["nutrition"]["minerals"]["iron"], "8 mg")?;
    assert_mass(
        &original_snapshot["nutrition"]["vitamins"]["vitamin_c"],
        "10 mg",
    )?;
    let null_input_patch = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"food", "amount":null}}),
        )?)
        .await?;
    assert_eq!(null_input_patch.status, StatusCode::BAD_REQUEST);
    assert_eq!(null_input_patch.json["code"], "invalid_input");

    let source_uri = format!("/foods/custom/{food_id}");
    let source_update = app
        .request(http_json_request(
            "PUT",
            &source_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({
                "description":"Updated wings",
                "parsed_data": {
                    "nutrition": {"per":"100 g", "energy":"300 kcal", "protein":"30 g"},
                    "serving_sizes": [{"name":"four wings", "quantity":"200 g"}]
                }
            }),
        )?)
        .await?;
    assert_eq!(
        source_update.status,
        StatusCode::OK,
        "{:?}",
        source_update.json
    );
    let unchanged = app.request(http_request(&log_uri)?).await?;
    assert_eq!(unchanged.json["data"], original_snapshot);
    let other_food_id = create_test_food(
        &app,
        "Other wings",
        json!({
            "nutrition": {"per":"100 g", "energy":"100 kcal"},
            "serving_sizes": [{"name":"one wing", "quantity":"50 g"}]
        }),
    )
    .await?;
    let incompatible_food = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"food", "food_id":other_food_id}}),
        )?)
        .await?;
    assert_eq!(incompatible_food.status, StatusCode::BAD_REQUEST);
    assert_eq!(incompatible_food.json["code"], "invalid_input");

    let refreshed = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"food", "amount":"2"}}),
        )?)
        .await?;
    assert_eq!(refreshed.status, StatusCode::OK, "{:?}", refreshed.json);
    assert_eq!(refreshed.json["data"]["description"], "Updated wings");
    assert_eq!(refreshed.json["data"]["quantity"], "400 g");
    assert_calories(
        &refreshed.json["data"]["nutrition"]["calories"],
        "1200 kcal",
    )?;
    assert_mass(&refreshed.json["data"]["nutrition"]["protein"], "120 g")?;
    assert_eq!(refreshed.json["data"]["input"]["amount"], "2");

    let deleted_source = app
        .request(http_method_request(
            "DELETE",
            &source_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(deleted_source.status, StatusCode::NO_CONTENT);
    let metadata_patch = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"day":"2025-08-02", "time":"10:30", "entry":{"type":"food"}}),
        )?)
        .await?;
    assert_eq!(
        metadata_patch.status,
        StatusCode::OK,
        "{:?}",
        metadata_patch.json
    );
    assert_eq!(metadata_patch.json["day"], "2025-08-02");
    assert_eq!(metadata_patch.json["time"], "10:30:00.000");
    assert_eq!(metadata_patch.json["data"], refreshed.json["data"]);

    let time_only_patch = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"time":"11:30"}),
        )?)
        .await?;
    assert_eq!(
        time_only_patch.status,
        StatusCode::OK,
        "{:?}",
        time_only_patch.json
    );
    assert_eq!(time_only_patch.json["data"], refreshed.json["data"]);

    let missing_source_patch = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"food", "amount":"3"}}),
        )?)
        .await?;
    assert_eq!(missing_source_patch.status, StatusCode::BAD_REQUEST);
    assert_eq!(missing_source_patch.json["code"], "invalid_input");
    Ok(())
}

#[tokio::test]
async fn rest_profile_logs_reject_invalid_food_inputs_and_allow_zero_calories()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let logs_uri = format!("/profiles/{profile_id}/logs");
    let mass_food_id = create_test_food(
        &app,
        "Mass food",
        json!({
            "nutrition": {"per":"100 g", "energy":"100 kcal"},
            "serving_sizes": [
                {"name":"piece", "quantity":"30 g"},
                {"name":"piece", "quantity":"40 g"}
            ]
        }),
    )
    .await?;
    let missing_energy_id = create_test_food(
        &app,
        "Food without calories",
        json!({"nutrition":{"per":"100 g", "protein":"5 g"}}),
    )
    .await?;
    let zero_calorie_id = create_test_food(
        &app,
        "Zero-calorie food",
        json!({"nutrition":{"per":"100 g", "energy":"0 kcal"}}),
    )
    .await?;

    for entry in [
        json!({"type":"food", "food_id":99999999, "amount":"1", "serving_size":"g"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"1", "serving_size":"not a serving"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"1", "serving_size":"piece"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"1", "serving_size":"Piece"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"1", "serving_size":"ml"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"0", "serving_size":"g"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":null, "serving_size":"g"}),
        json!({"type":"food", "food_id":null, "amount":"1", "serving_size":"g"}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"1", "serving_size":null}),
        json!({"type":"food", "food_id":mass_food_id, "amount":"1", "serving_size":"g", "description":"forged"}),
    ] {
        let response = create_food_log(&app, &logs_uri, "2025-08-03", entry).await?;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{:?}",
            response.json
        );
        assert_eq!(response.json["code"], "invalid_input");
    }

    let missing_energy = create_food_log(
        &app,
        &logs_uri,
        "2025-08-03",
        json!({"type":"food", "food_id":missing_energy_id, "amount":"1", "serving_size":"g"}),
    )
    .await?;
    assert_eq!(missing_energy.status, StatusCode::BAD_REQUEST);
    assert_eq!(missing_energy.json["code"], "invalid_input");
    let message = missing_energy.json["error"]
        .as_str()
        .ok_or("missing-calorie error has no message")?
        .to_lowercase();
    assert!(message.contains("calor"));
    assert!(message.contains("edit") || message.contains("create"));

    let zero_calories = create_food_log(
        &app,
        &logs_uri,
        "2025-08-03",
        json!({"type":"food", "food_id":zero_calorie_id, "amount":"1", "serving_size":"g"}),
    )
    .await?;
    assert_eq!(
        zero_calories.status,
        StatusCode::CREATED,
        "{:?}",
        zero_calories.json
    );
    assert_calories(
        &zero_calories.json["data"]["nutrition"]["calories"],
        "0 kcal",
    )?;
    Ok(())
}

#[tokio::test]
async fn rest_profile_logs_enforce_scope_and_hide_other_profiles_and_logs()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let logs_uri = format!("/profiles/{profile_id}/logs");
    let created = create_log(&app, &logs_uri, Some("07:30"), "2025-02-01", "72 kg").await?;
    let log_id = created.json["id"].as_i64().ok_or("created log has no ID")?;
    let log_uri = format!("{logs_uri}/{log_id}");
    let food_id = create_test_food(
        &app,
        "Shared fruit",
        json!({"nutrition":{"per":"100 g", "energy":"90 kcal"}}),
    )
    .await?;
    let food_created = create_food_log(
        &app,
        &logs_uri,
        "2025-02-01",
        json!({"type":"food", "food_id":food_id, "amount":"100", "serving_size":"g"}),
    )
    .await?;
    let food_log_id = food_created.json["id"]
        .as_i64()
        .ok_or("created food log has no ID")?;
    let food_log_uri = format!("{logs_uri}/{food_log_id}");

    let read_list = app.request(http_request(&logs_uri)?).await?;
    assert_eq!(read_list.status, StatusCode::OK);
    let read_get = app.request(http_request(&log_uri)?).await?;
    assert_eq!(read_get.status, StatusCode::OK);
    for response in [
        app.request(http_json_request(
            "POST",
            &logs_uri,
            TEST_REST_ACCESS_TOKEN,
            log_input("2025-02-01", None, "73 kg"),
        )?)
        .await?,
        app.request(http_json_request(
            "POST",
            &logs_uri,
            TEST_REST_ACCESS_TOKEN,
            json!({
                "day":"2025-02-01",
                "entry":{"type":"food", "food_id":food_id, "amount":"100", "serving_size":"g"}
            }),
        )?)
        .await?,
        app.request(http_json_request(
            "PATCH",
            &log_uri,
            TEST_REST_ACCESS_TOKEN,
            json!({"day":"2025-02-02"}),
        )?)
        .await?,
        app.request(http_method_request(
            "DELETE",
            &log_uri,
            TEST_REST_ACCESS_TOKEN,
        )?)
        .await?,
    ] {
        assert_eq!(response.status, StatusCode::FORBIDDEN);
    }

    let other_token = seed_other_access_token(
        &app,
        false,
        OTHER_REST_TOKEN,
        OTHER_REST_REFRESH,
        OTHER_REST_CLIENT,
    )
    .await?;
    let (another_profile_id, _) =
        create_test_profile(&app, "Cristi", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let hidden_by_profile = app
        .request(http_request(&format!(
            "/profiles/{another_profile_id}/logs/{food_log_id}"
        ))?)
        .await?;
    assert_eq!(hidden_by_profile.status, StatusCode::NOT_FOUND);
    let hidden_list = app
        .request(http_request_with_token(&logs_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_list.status, StatusCode::NOT_FOUND);
    let hidden_get = app
        .request(http_request_with_token(&log_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_get.status, StatusCode::NOT_FOUND);
    let hidden_food_get = app
        .request(http_request_with_token(&food_log_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_food_get.status, StatusCode::NOT_FOUND);
    let hidden_patch = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            &other_token,
            json!({"day":"2025-02-03"}),
        )?)
        .await?;
    assert_eq!(hidden_patch.status, StatusCode::NOT_FOUND);
    let hidden_food_patch = app
        .request(http_json_request(
            "PATCH",
            &food_log_uri,
            &other_token,
            json!({"day":"2025-02-03"}),
        )?)
        .await?;
    assert_eq!(hidden_food_patch.status, StatusCode::NOT_FOUND);
    let hidden_delete = app
        .request(http_method_request("DELETE", &log_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_delete.status, StatusCode::NOT_FOUND);
    let hidden_food_delete = app
        .request(http_method_request("DELETE", &food_log_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_food_delete.status, StatusCode::NOT_FOUND);
    let hidden_create = app
        .request(http_json_request(
            "POST",
            &logs_uri,
            &other_token,
            log_input("2025-02-03", None, "74 kg"),
        )?)
        .await?;
    assert_eq!(hidden_create.status, StatusCode::NOT_FOUND);
    Ok(())
}

async fn create_log(
    app: &TestApplication,
    uri: &str,
    time: Option<&str>,
    day: &str,
    weight: &str,
) -> Result<crate::tests::ResponseBody, Box<dyn Error>> {
    let mut body = log_input(day, time, weight);
    app.request(http_json_request(
        "POST",
        uri,
        TEST_REST_WRITE_ACCESS_TOKEN,
        body.take(),
    )?)
    .await
}

async fn create_food_log(
    app: &TestApplication,
    uri: &str,
    day: &str,
    entry: Value,
) -> Result<crate::tests::ResponseBody, Box<dyn Error>> {
    app.request(http_json_request(
        "POST",
        uri,
        TEST_REST_WRITE_ACCESS_TOKEN,
        json!({"day":day, "entry":entry}),
    )?)
    .await
}

fn log_input(day: &str, time: Option<&str>, weight: &str) -> Value {
    let mut input = json!({
        "day": day,
        "entry": {
            "type": "biometric",
            "measurement": {"type": "weight", "value": weight}
        }
    });
    if let Some(time) = time {
        input["time"] = json!(time);
    }
    input
}

async fn seed_logs_for_pagination(
    app: &TestApplication,
    profile_id: i64,
    count: usize,
) -> Result<(), Box<dyn Error>> {
    let username = Username::new("alice")?;
    let user: UserId = app
        ._authorization_database
        .user_by_username(&username)
        .await?
        .ok_or("test authorization user is missing")?
        .user
        .id;
    let profile_id = ProfileId::new(profile_id)?;
    for index in 0..count {
        let input: CreateLog = serde_json::from_value(log_input(
            "2025-06-03",
            Some("08:00"),
            &format!("{} kg", 50 + index),
        ))?;
        app.database
            .create_profile_log(user, profile_id, &input)
            .await?;
    }
    Ok(())
}

fn assert_local_time_near_now(time: &str, offset_seconds: u64) -> Result<(), Box<dyn Error>> {
    let current_utc = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let expected = ((current_utc + offset_seconds) % 86_400) as u32;
    let (hour, remainder) = time
        .split_once(':')
        .ok_or("invalid serialized local time")?;
    let (minute, second) = remainder
        .split_once(':')
        .ok_or("invalid serialized local time")?;
    let second = second
        .split('.')
        .next()
        .ok_or("invalid serialized local time")?;
    let actual =
        hour.parse::<u32>()? * 3_600 + minute.parse::<u32>()? * 60 + second.parse::<u32>()?;
    let direct = expected.abs_diff(actual);
    let around_midnight = 86_400 - direct;
    assert!(
        direct.min(around_midnight) <= 5,
        "{time:?} is not near current local time"
    );
    Ok(())
}

#[tokio::test]
async fn journal_rest_round_trip_and_filter() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) =
        create_test_profile(&app, "Journal", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let (other_id, _) = create_test_profile(&app, "Other", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let uri = format!("/profiles/{profile_id}/logs");
    let original = "  Café\n今日  ";
    let create = |text| json!({"day":"2025-06-01","entry":{"type":"journal","text":text}});
    let first = app
        .request(http_json_request(
            "POST",
            &uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            create(original),
        )?)
        .await?;
    assert_eq!(first.status, StatusCode::CREATED, "{:?}", first.json);
    assert_eq!(
        first.json["data"],
        json!({"type":"journal","text":original})
    );
    let id = first.json["id"].as_i64().ok_or("journal ID is missing")?;
    let item_uri = format!("{uri}/{id}");
    let second = app
        .request(http_json_request(
            "POST",
            &uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            create("Second"),
        )?)
        .await?;
    assert_eq!(second.status, StatusCode::CREATED);
    let blank = app
        .request(http_json_request(
            "POST",
            &uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            create(" \n\t"),
        )?)
        .await?;
    assert_eq!(blank.status, StatusCode::BAD_REQUEST);
    let fetched = app.request(http_request(&item_uri)?).await?;
    assert_eq!(fetched.json["data"]["text"], original);
    let wrong_profile = app
        .request(http_request(&format!("/profiles/{other_id}/logs/{id}"))?)
        .await?;
    assert_eq!(wrong_profile.status, StatusCode::NOT_FOUND);
    let listed = app
        .request(http_request(&format!("{uri}?entry_type=journal"))?)
        .await?;
    assert_eq!(listed.json["items"].as_array().map(Vec::len), Some(2));
    let invalid_filter = app
        .request(http_request(&format!(
            "{uri}?entry_type=journal&subtype=weight"
        ))?)
        .await?;
    assert_eq!(invalid_filter.status, StatusCode::BAD_REQUEST);
    let moved = app
        .request(http_json_request(
            "PATCH",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"day":"2025-06-02"}),
        )?)
        .await?;
    assert_eq!(moved.json["data"]["text"], original);
    let updated = app
        .request(http_json_request(
            "PATCH",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"journal","text":" revised "}}),
        )?)
        .await?;
    assert_eq!(updated.json["data"]["text"], " revised ");
    let rejected = app
        .request(http_json_request(
            "PATCH",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"biometric","measurement":{"type":"weight","value":"80 kg"}}}),
        )?)
        .await?;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
    let denied = app
        .request(http_json_request(
            "PATCH",
            &item_uri,
            TEST_REST_ACCESS_TOKEN,
            json!({"day":"2025-06-03"}),
        )?)
        .await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let deleted = app
        .request(http_method_request(
            "DELETE",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert!(deleted.status.is_success());
    let missing = app.request(http_request(&item_uri)?).await?;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn exercise_rest_round_trip_and_filter() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) =
        create_test_profile(&app, "Exercise", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let (other_id, _) = create_test_profile(&app, "Other", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let uri = format!("/profiles/{profile_id}/logs");
    let entry = json!({"type":"exercise", "exercise_type":"walking", "intensity":"moderate", "duration":"30.5 min", "calories_burned":"150 kcal", "notes":"Outside"});
    let created = app
        .request(http_json_request(
            "POST",
            &uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"day":"2025-06-01", "entry":entry}),
        )?)
        .await?;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json);
    let mut expected = entry.clone();
    expected["duration"] = json!("1830 s");
    assert_eq!(created.json["data"], expected);
    let id = created.json["id"]
        .as_i64()
        .ok_or("exercise ID is missing")?;
    let item_uri = format!("{uri}/{id}");
    let fetched = app.request(http_request(&item_uri)?).await?;
    assert_eq!(fetched.json["data"], expected);
    let wrong_profile = app
        .request(http_request(&format!("/profiles/{other_id}/logs/{id}"))?)
        .await?;
    assert_eq!(wrong_profile.status, StatusCode::NOT_FOUND);
    let listed = app
        .request(http_request(&format!("{uri}?entry_type=exercise"))?)
        .await?;
    assert_eq!(listed.json["items"].as_array().map(Vec::len), Some(1));
    let invalid_filter = app
        .request(http_request(&format!(
            "{uri}?entry_type=exercise&subtype=weight"
        ))?)
        .await?;
    assert_eq!(invalid_filter.status, StatusCode::BAD_REQUEST);
    let updated = app
        .request(http_json_request(
            "PATCH",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"exercise", "duration":"45 min", "notes":null}}),
        )?)
        .await?;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(
        updated.json["data"],
        json!({"type":"exercise", "exercise_type":"walking", "intensity":"moderate", "duration":"2700 s", "calories_burned":"150 kcal", "notes":null})
    );
    let invalid = app
        .request(http_json_request(
            "PATCH",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"entry":{"type":"exercise", "intensity":null}}),
        )?)
        .await?;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    Ok(())
}
