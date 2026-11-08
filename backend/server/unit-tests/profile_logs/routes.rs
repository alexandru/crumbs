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

use super::{create_test_profile, seed_other_access_token, seed_profile_owner};

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
async fn rest_profile_logs_enforce_scope_and_hide_other_profiles_and_logs()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let logs_uri = format!("/profiles/{profile_id}/logs");
    let created = create_log(&app, &logs_uri, Some("07:30"), "2025-02-01", "72 kg").await?;
    let log_id = created.json["id"].as_i64().ok_or("created log has no ID")?;
    let log_uri = format!("{logs_uri}/{log_id}");

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
    let hidden_list = app
        .request(http_request_with_token(&logs_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_list.status, StatusCode::NOT_FOUND);
    let hidden_get = app
        .request(http_request_with_token(&log_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_get.status, StatusCode::NOT_FOUND);
    let hidden_patch = app
        .request(http_json_request(
            "PATCH",
            &log_uri,
            &other_token,
            json!({"day":"2025-02-03"}),
        )?)
        .await?;
    assert_eq!(hidden_patch.status, StatusCode::NOT_FOUND);
    let hidden_delete = app
        .request(http_method_request("DELETE", &log_uri, &other_token)?)
        .await?;
    assert_eq!(hidden_delete.status, StatusCode::NOT_FOUND);
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
