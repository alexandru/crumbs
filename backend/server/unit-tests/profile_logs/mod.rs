mod routes;

use std::error::Error;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::oauth::tests::{TEST_MCP_RESOURCE, TEST_REST_RESOURCE};
use crate::tests::{TestApplication, http_json_request};
use axum::http::StatusCode;
use crumbs_database::identity::{
    Approval, AuthorizationGrant, ClientAuthentication, CodeRedemption, CredentialHash,
    OAuthClient, OAuthScope, OAuthStore, PasswordHash, PendingAuthorization, TokenPair, Username,
    Users,
};
use crumbs_database::profile_logs::ProfileLogError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
