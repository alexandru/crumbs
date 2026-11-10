use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use crumbs_database::identity::{
    Approval, AuthorizationGrant, ClientAuthentication, CodeRedemption, CredentialHash,
    OAuthClient, OAuthScope, OAuthStore, PasswordHash, PendingAuthorization, TokenPair, Username,
    Users,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::oauth::tests::{
    TEST_MCP_RESOURCE, TEST_REST_ACCESS_TOKEN, TEST_REST_RESOURCE, TEST_REST_WRITE_ACCESS_TOKEN,
};
use crate::tests::{
    TestApplication, http_json_request, http_method_request, http_request, http_request_with_token,
};

const OTHER_USER_ACCESS_TOKEN: &str = "test-other-profile-user-access-token";
const OTHER_USER_REFRESH_TOKEN: &str = "test-other-profile-user-refresh-token";
const OTHER_USER_CLIENT_ID: &str = "test-other-profile-user-client";
pub(super) const OTHER_MCP_USER_ACCESS_TOKEN: &str = "test-other-mcp-profile-user-access-token";
const OTHER_MCP_USER_REFRESH_TOKEN: &str = "test-other-mcp-profile-user-refresh-token";
const OTHER_MCP_USER_CLIENT_ID: &str = "test-other-mcp-profile-user-client";

#[tokio::test]
async fn rest_profiles_support_defaults_custom_calories_and_patch_nulls()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_account(&app).await?;
    let created = app
        .request(http_json_request(
            "POST",
            "/profiles",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({
                "name": " Alex ",
                "settings": {
                    "weight_goal": "60 kg",
                    "bmr": {"mode": "custom", "value": "1700 kcal"},
                    "baseline_activity": {"mode": "custom", "value": "400 kcal"}
                }
            }),
        )?)
        .await?;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json);
    let profile_id = created.json["id"].as_i64().ok_or("profile has no ID")?;
    assert_eq!(created.json["name"], "Alex");
    assert_eq!(created.json["data"]["settings"]["timezone"], "UTC");
    assert_eq!(created.json["data"]["settings"]["weight_goal"], "60 kg");
    assert_eq!(
        created.json["data"]["settings"]["bmr"],
        json!({"mode": "custom", "value": "1700 kcal"})
    );
    assert_eq!(
        created.json["data"]["settings"]["baseline_activity"],
        json!({"mode": "custom", "value": "400 kcal"})
    );
    assert_eq!(created.json["data"]["cache"]["current_weight"], Value::Null);
    assert_eq!(created.json["derived"]["bmr"], "1700 kcal");
    assert_eq!(created.json["derived"]["baseline_activity"], "400 kcal");
    assert_eq!(created.json["derived"]["maintenance_calories"], "2100 kcal");
    assert_eq!(
        created
            .headers
            .get(axum::http::header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/profiles/{profile_id}").as_str())
    );

    let duplicate = app
        .request(http_json_request(
            "POST",
            "/profiles",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"name": "  aLeX  "}),
        )?)
        .await?;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);
    assert_eq!(duplicate.json["code"], "duplicate_name");

    let profile_uri = format!("/profiles/{profile_id}");
    let get = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(get.status, StatusCode::OK);
    assert_eq!(get.json, created.json);

    let omitted_patch = app
        .request(http_json_request(
            "PATCH",
            &profile_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"weight_goal": null}),
        )?)
        .await?;
    assert_eq!(
        omitted_patch.status,
        StatusCode::OK,
        "{:?}",
        omitted_patch.json
    );
    assert_eq!(omitted_patch.json["name"], "Alex");
    assert_eq!(
        omitted_patch.json["data"]["settings"]["weight_goal"],
        Value::Null
    );
    assert_eq!(
        omitted_patch.json["data"]["settings"]["bmr"],
        json!({"mode": "custom", "value": "1700 kcal"})
    );
    assert_eq!(
        omitted_patch.json["derived"]["maintenance_calories"],
        "2100 kcal"
    );

    let reset_bmr = app
        .request(http_json_request(
            "PATCH",
            &profile_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"bmr": null}),
        )?)
        .await?;
    assert_eq!(reset_bmr.status, StatusCode::OK, "{:?}", reset_bmr.json);
    assert_eq!(
        reset_bmr.json["data"]["settings"]["bmr"],
        json!({"mode": "calculated"})
    );
    assert_eq!(reset_bmr.json["derived"]["bmr"], Value::Null);
    assert_eq!(
        reset_bmr.json["derived"]["maintenance_calories"],
        Value::Null
    );

    let null_name = app
        .request(http_json_request(
            "PATCH",
            &profile_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"name": null}),
        )?)
        .await?;
    assert_eq!(null_name.status, StatusCode::BAD_REQUEST);
    assert_eq!(null_name.json["code"], "invalid_input");

    let defaults = app
        .request(http_json_request(
            "POST",
            "/profiles",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"name": "Cristi"}),
        )?)
        .await?;
    assert_eq!(defaults.status, StatusCode::CREATED, "{:?}", defaults.json);
    assert_eq!(defaults.json["data"]["settings"]["timezone"], "UTC");
    assert_eq!(
        defaults.json["data"]["settings"]["bmr"],
        json!({"mode": "calculated"})
    );
    assert_eq!(
        defaults.json["data"]["settings"]["baseline_activity"],
        Value::Null
    );
    assert_eq!(defaults.json["derived"]["bmr"], Value::Null);

    let listed = app.request(http_request("/profiles?limit=1")?).await?;
    assert_eq!(listed.status, StatusCode::OK, "{:?}", listed.json);
    assert_eq!(listed.json["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed.json["items"][0]["id"], profile_id);
    let cursor = listed.json["last_id"]
        .as_i64()
        .ok_or("list has no cursor")?;
    let next_page = app
        .request(http_request(&format!(
            "/profiles?last_id={cursor}&limit=100"
        ))?)
        .await?;
    assert_eq!(next_page.status, StatusCode::OK, "{:?}", next_page.json);
    assert_eq!(next_page.json["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(next_page.json["items"][0]["name"], "Cristi");
    Ok(())
}

#[tokio::test]
async fn rest_profiles_are_account_scoped_and_require_write_scope_for_changes()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_account(&app).await?;
    let created = app
        .request(http_json_request(
            "POST",
            "/profiles",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"name": "Alex"}),
        )?)
        .await?;
    let profile_id = created.json["id"].as_i64().ok_or("profile has no ID")?;
    let profile_uri = format!("/profiles/{profile_id}");

    let other_token = seed_other_user_access(
        &app,
        TEST_REST_RESOURCE,
        OTHER_USER_ACCESS_TOKEN,
        OTHER_USER_REFRESH_TOKEN,
        OTHER_USER_CLIENT_ID,
    )
    .await?;
    let missing_get = app
        .request(http_request_with_token(&profile_uri, &other_token)?)
        .await?;
    assert_eq!(missing_get.status, StatusCode::NOT_FOUND);
    assert_eq!(missing_get.json["code"], "not_found");

    let missing_patch = app
        .request(http_json_request(
            "PATCH",
            &profile_uri,
            &other_token,
            json!({"name": "Cannot see Alex"}),
        )?)
        .await?;
    assert_eq!(missing_patch.status, StatusCode::NOT_FOUND);
    assert_eq!(missing_patch.json["code"], "not_found");

    let duplicate_name_for_other_account = app
        .request(http_json_request(
            "POST",
            "/profiles",
            &other_token,
            json!({"name": "Alex"}),
        )?)
        .await?;
    assert_eq!(duplicate_name_for_other_account.status, StatusCode::CREATED);
    let other_list = app
        .request(http_request_with_token("/profiles", &other_token)?)
        .await?;
    assert_eq!(other_list.json["items"].as_array().map(Vec::len), Some(1));

    let read_only_create = app
        .request(http_json_request(
            "POST",
            "/profiles",
            TEST_REST_ACCESS_TOKEN,
            json!({"name": "Denied create"}),
        )?)
        .await?;
    assert_eq!(read_only_create.status, StatusCode::FORBIDDEN);
    let read_only_patch = app
        .request(http_json_request(
            "PATCH",
            &profile_uri,
            TEST_REST_ACCESS_TOKEN,
            json!({"name": "Denied patch"}),
        )?)
        .await?;
    assert_eq!(read_only_patch.status, StatusCode::FORBIDDEN);

    let delete = app
        .request(http_method_request(
            "DELETE",
            &profile_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(delete.status, StatusCode::METHOD_NOT_ALLOWED);
    Ok(())
}

pub(in crate::profiles) async fn seed_other_mcp_user_access(
    app: &TestApplication,
) -> Result<String, Box<dyn Error>> {
    seed_other_user_access(
        app,
        TEST_MCP_RESOURCE,
        OTHER_MCP_USER_ACCESS_TOKEN,
        OTHER_MCP_USER_REFRESH_TOKEN,
        OTHER_MCP_USER_CLIENT_ID,
    )
    .await
}

pub(in crate::profiles) async fn seed_profile_account(
    app: &TestApplication,
) -> Result<(), Box<dyn Error>> {
    let username = Username::new("alice")?;
    let credentials = app
        ._authorization_database
        .user_by_username(&username)
        .await?
        .ok_or("test authorization user is missing")?;
    let content_user = app
        .database
        .create_user(&username, &credentials.password_hash)
        .await?;
    if content_user.id != credentials.user.id {
        return Err("test user IDs differ between content and authorization databases".into());
    }
    Ok(())
}

async fn seed_other_user_access(
    app: &TestApplication,
    resource: &str,
    access_token: &str,
    refresh_token: &str,
    client_id: &str,
) -> Result<String, Box<dyn Error>> {
    let store = app._authorization_database.as_ref();
    let username = Username::new("other-profile-user")?;
    let password_hash = PasswordHash::hash("test other profile user password")?;
    let content_user = app.database.create_user(&username, &password_hash).await?;
    let user = store.create_user(&username, &password_hash).await?;
    if content_user.id != user.id {
        return Err("test user IDs differ between content and authorization databases".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    let request_hash = credential_hash("other-profile-request");
    let csrf_hash = credential_hash("other-profile-csrf");
    let code_hash = credential_hash("other-profile-code");
    let challenge = "profile-test-pkce-challenge".to_owned();
    let redirect_uri = "https://client.example/callback".to_owned();
    let client = OAuthClient {
        id: client_id.to_owned(),
        name: "Other profile test client".to_owned(),
        redirect_uris: vec![redirect_uri.clone()],
        authentication: ClientAuthentication::Public,
        allow_refresh: true,
    };
    store.register_client(&client, now).await?;
    store
        .begin_authorization(
            &PendingAuthorization {
                request_hash,
                csrf_hash,
                grant: AuthorizationGrant {
                    client_id: client.id.clone(),
                    redirect_uri: redirect_uri.clone(),
                    resource: resource.to_owned(),
                    scope: OAuthScope::ReadWrite,
                    state: None,
                    code_challenge: challenge.clone(),
                },
                expires_at: now + 600_000,
            },
            now,
        )
        .await?;
    store
        .approve_authorization(
            &Approval {
                request_hash,
                csrf_hash,
                user_id: user.id,
                password_hash,
                code_hash,
                code_expires_at: now + 60_000,
            },
            now,
        )
        .await?;
    store
        .redeem_code(
            &CodeRedemption {
                code_hash,
                client_id: client.id,
                redirect_uri,
                resource: resource.to_owned(),
                code_challenge: challenge,
            },
            None,
            &TokenPair {
                access_hash: credential_hash(access_token),
                refresh_hash: credential_hash(refresh_token),
                access_expires_at: now + 600_000,
                session_expires_at: now + 2_592_000_000,
            },
            now,
        )
        .await?;
    Ok(access_token.to_owned())
}

fn credential_hash(value: &str) -> CredentialHash {
    CredentialHash::new(Sha256::digest(value.as_bytes()).into())
}
