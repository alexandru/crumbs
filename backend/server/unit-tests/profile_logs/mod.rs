mod mcp;
mod routes;

use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use crumbs_database::identity::{
    Approval, AuthorizationGrant, ClientAuthentication, CodeRedemption, CredentialHash,
    OAuthClient, OAuthScope, OAuthStore, PasswordHash, PendingAuthorization, TokenPair, Username,
    Users,
};
use crumbs_database::nutrition_model::{Kilocalories, Mass, Reported};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::oauth::tests::{TEST_MCP_RESOURCE, TEST_REST_RESOURCE, TEST_REST_WRITE_ACCESS_TOKEN};
use crate::tests::{TestApplication, http_json_request};

pub(super) async fn seed_profile_owner(app: &TestApplication) -> Result<(), Box<dyn Error>> {
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
        .parse::<Reported<Kilocalories>>()?;
    let expected = expected.parse::<Reported<Kilocalories>>()?;
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
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    let request_hash = credential_hash("profile-log-test-request");
    let csrf_hash = credential_hash("profile-log-test-csrf");
    let code_hash = credential_hash("profile-log-test-code");
    let challenge = "profile-log-test-pkce-challenge".to_owned();
    let resource = if mcp {
        TEST_MCP_RESOURCE
    } else {
        TEST_REST_RESOURCE
    };
    let redirect_uri = "https://client.example/callback".to_owned();
    let client = OAuthClient {
        id: client_id.to_owned(),
        name: "Profile log test client".to_owned(),
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
                access_hash: credential_hash(token),
                refresh_hash: credential_hash(refresh_token),
                access_expires_at: now + 600_000,
                session_expires_at: now + 2_592_000_000,
            },
            now,
        )
        .await?;
    Ok(token.to_owned())
}

fn credential_hash(value: &str) -> CredentialHash {
    CredentialHash::new(Sha256::digest(value.as_bytes()).into())
}
