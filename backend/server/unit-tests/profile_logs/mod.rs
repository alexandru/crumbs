mod routes;

use std::error::Error;
use std::io;

use axum::http::StatusCode;
use crumbs_database::identity::{OAuthScope, PasswordHash, Username, Users};
use crumbs_database::nutrition_model::{Kcal, Mass, Reported};
use crumbs_database::profile_logs::ProfileLogError;
use serde_json::{Value, json};

use crate::oauth::tests::{TEST_MCP_RESOURCE, TEST_REST_RESOURCE, TEST_REST_WRITE_ACCESS_TOKEN};
use crate::tests::{TestApplication, http_json_request};

pub(super) struct ProfileLogErrorCase {
    pub(super) name: &'static str,
    pub(super) error: ProfileLogError,
    pub(super) status: StatusCode,
    pub(super) code: &'static str,
    pub(super) message: &'static str,
}

pub(super) fn profile_log_error_cases() -> [ProfileLogErrorCase; 4] {
    [
        ProfileLogErrorCase {
            name: "invalid input",
            error: ProfileLogError::InvalidInput("weight must be positive".to_owned()),
            status: StatusCode::BAD_REQUEST,
            code: "invalid_input",
            message: "weight must be positive",
        },
        ProfileLogErrorCase {
            name: "invalid stored data",
            error: ProfileLogError::InvalidStoredData,
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "invalid_stored_data",
            message: "profile log operation failed",
        },
        ProfileLogErrorCase {
            name: "storage failure",
            error: ProfileLogError::Storage(Box::new(io::Error::other(
                "private storage source detail",
            ))),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "storage",
            message: "profile log operation failed",
        },
        ProfileLogErrorCase {
            name: "clock failure",
            error: ProfileLogError::Clock(Box::new(io::Error::other(
                "private clock source detail",
            ))),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "clock",
            message: "profile log operation failed",
        },
    ]
}

pub(super) async fn create_test_profile(
    app: &TestApplication,
    name: &str,
    access_token: &str,
) -> Result<(i64, Value), Box<dyn Error>> {
    let response = app
        .request(http_json_request(
            "POST",
            "/profiles",
            access_token,
            json!({
                "name": name,
                "settings": {
                    "birth_date": "1990-01-01",
                    "sex": "male",
                    "height": "180 cm",
                    "timezone": "Asia/Kolkata",
                    "weight_goal": "75 kg",
                    "baseline_activity": {"mode": "preset", "level": "sedentary"}
                }
            }),
        )?)
        .await?;
    if !response.status.is_success() {
        return Err(format!("profile creation failed: {:?}", response.json).into());
    }
    let id = response.json["id"]
        .as_i64()
        .ok_or("created profile has no ID")?;
    Ok((id, response.json))
}

pub(super) async fn create_test_food(
    app: &TestApplication,
    description: &str,
    parsed_data: Value,
) -> Result<i64, Box<dyn Error>> {
    let response = app
        .request(http_json_request(
            "POST",
            "/foods/custom",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"description":description, "parsed_data":parsed_data}),
        )?)
        .await?;
    if !response.status.is_success() {
        return Err(format!("custom food creation failed: {:?}", response.json).into());
    }
    response.json["id"]
        .as_i64()
        .ok_or_else(|| "created custom food has no ID".into())
}

pub(super) fn assert_calories(actual: &Value, expected: &str) -> Result<(), Box<dyn Error>> {
    let actual = actual
        .as_str()
        .ok_or("calorie value is not serialized as a string")?
        .parse::<Reported<Kcal>>()?;
    let expected = expected.parse::<Reported<Kcal>>()?;
    if actual != expected {
        return Err(format!("expected {expected}, received {actual}").into());
    }
    Ok(())
}

pub(super) fn assert_mass(actual: &Value, expected: &str) -> Result<(), Box<dyn Error>> {
    let actual = actual
        .as_str()
        .ok_or("mass value is not serialized as a string")?
        .parse::<Reported<Mass>>()?;
    let expected = expected.parse::<Reported<Mass>>()?;
    if actual != expected {
        return Err(format!("expected {expected}, received {actual}").into());
    }
    Ok(())
}

pub(super) async fn seed_other_access_token(
    app: &TestApplication,
    mcp: bool,
    token: &str,
    refresh_token: &str,
    client_id: &str,
) -> Result<String, Box<dyn Error>> {
    let store = app._authorization_database.as_ref();
    let username = Username::new("other-log-user")?;
    let password_hash = PasswordHash::hash("test profile log user password")?;
    let content_user = app.database.create_user(&username, &password_hash).await?;
    let user = store.create_user(&username, &password_hash).await?;
    if content_user.id != user.id {
        return Err("test user IDs differ between content and authorization databases".into());
    }
    let resource = if mcp {
        TEST_MCP_RESOURCE
    } else {
        TEST_REST_RESOURCE
    };
    crate::oauth::tests::seed_session_for_user(
        store,
        client_id,
        resource,
        token,
        refresh_token,
        OAuthScope::ReadWrite,
        (user.id, password_hash),
    )
    .await?;
    Ok(token.to_owned())
}
