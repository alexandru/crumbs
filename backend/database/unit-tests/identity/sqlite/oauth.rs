use std::error::Error as StdError;
use std::io;

use super::*;
use crate::DatabaseConfig;
use crate::identity::{PasswordHash, Username, Users};
use crate::runtime::Database;

const PASSWORD_HASH: &str = "$2a$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy";
const NOW: AuthTimestamp = 1_000;

#[tokio::test]
async fn client_registration_persists_hashed_authentication_and_capabilities()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = OAuthClient {
        id: "confidential-client".to_owned(),
        name: "Confidential client".to_owned(),
        redirect_uris: vec!["https://client.example/callback".to_owned()],
        authentication: ClientAuthentication::SecretBasic(hash(1)),
        allow_refresh: false,
    };
    database.register_client(&client, NOW).await?;
    let stored = database
        .oauth_client(&client.id)
        .await?
        .ok_or_else(|| io::Error::other("registered client was not found"))?;
    assert_eq!(stored.id, client.id);
    assert_eq!(stored.name, client.name);
    assert_eq!(stored.redirect_uris, client.redirect_uris);
    assert!(!stored.allow_refresh);
    assert!(matches!(
        stored.authentication,
        ClientAuthentication::SecretBasic(secret_hash) if secret_hash == hash(1)
    ));
    assert!(matches!(
        database.register_client(&client, NOW).await,
        Err(AuthStoreError::InvalidRecord)
    ));
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn authorization_bindings_are_checked_and_code_replay_revokes_the_family()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("client-a");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;
    let authorization = pending(hash(10), hash(11), &client.id, NOW, 2_000);
    database.begin_authorization(&authorization, NOW).await?;
    assert!(
        database
            .pending_authorization(&authorization.request_hash, NOW)
            .await?
            .is_some()
    );
    let approval = Approval {
        request_hash: authorization.request_hash,
        csrf_hash: authorization.csrf_hash,
        user_id,
        password_hash,
        code_hash: hash(12),
        code_expires_at: 1_100,
    };

    let wrong_csrf = Approval {
        csrf_hash: hash(99),
        ..approval.clone()
    };
    assert!(
        database
            .approve_authorization(&wrong_csrf, NOW + 10)
            .await?
            .is_none()
    );
    let approved = database
        .approve_authorization(&approval, NOW + 10)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization request was rejected"))?;
    assert!(
        database
            .approve_authorization(&approval, NOW + 11)
            .await?
            .is_none()
    );
    let redemption = CodeRedemption {
        code_hash: approval.code_hash,
        client_id: approved.client_id,
        redirect_uri: approved.redirect_uri,
        resource: approved.resource,
        code_challenge: approved.code_challenge,
    };

    let tokens = token_pair(20, 21, 1_180, 1_500);
    for invalid in [
        CodeRedemption {
            client_id: "other-client".to_owned(),
            ..redemption.clone()
        },
        CodeRedemption {
            redirect_uri: "https://other.example/callback".to_owned(),
            ..redemption.clone()
        },
        CodeRedemption {
            resource: "https://other-api.example".to_owned(),
            ..redemption.clone()
        },
        CodeRedemption {
            code_challenge: "other-challenge".to_owned(),
            ..redemption.clone()
        },
    ] {
        assert!(
            database
                .redeem_code(&invalid, None, &tokens, NOW + 20)
                .await?
                .is_none()
        );
    }

    let extra_scope = OAuthScope::Write;
    assert!(matches!(
        database
            .redeem_code(&redemption, Some(&extra_scope), &tokens, NOW + 20)
            .await,
        Err(AuthStoreError::InvalidScope)
    ));

    let equivalent_scope = OAuthScope::parse("crumbs:read crumbs:read")
        .ok_or_else(|| io::Error::other("duplicate read scope was rejected"))?;
    let session = database
        .redeem_code(&redemption, Some(&equivalent_scope), &tokens, NOW + 20)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;
    assert_eq!(session.user_id, user_id);
    assert_eq!(session.client_id, client.id);
    assert_eq!(session.expires_at, 1_500);
    assert!(
        database
            .authenticate_access(&tokens.access_hash, &session.resource, NOW + 21)
            .await?
            .is_some()
    );

    let replay_tokens = token_pair(30, 31, 1_300, 1_600);
    assert!(
        database
            .redeem_code(&redemption, None, &replay_tokens, 1_200)
            .await?
            .is_none()
    );
    assert!(
        database
            .authenticate_access(&tokens.access_hash, &session.resource, NOW + 22)
            .await?
            .is_none()
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn readwrite_scope_order_is_normalized_for_redemption_and_refresh()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("readwrite-client");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;
    let authorization = PendingAuthorization {
        request_hash: hash(50),
        csrf_hash: hash(51),
        grant: AuthorizationGrant {
            client_id: client.id.clone(),
            redirect_uri: "https://client.example/callback".to_owned(),
            resource: "https://api.example".to_owned(),
            scope: OAuthScope::ReadWrite,
            state: None,
            code_challenge: "readwrite-challenge".to_owned(),
        },
        expires_at: 2_000,
    };
    database.begin_authorization(&authorization, NOW).await?;
    let approval = Approval {
        request_hash: authorization.request_hash,
        csrf_hash: authorization.csrf_hash,
        user_id,
        password_hash,
        code_hash: hash(52),
        code_expires_at: 1_100,
    };
    let approved = database
        .approve_authorization(&approval, NOW + 10)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization request was rejected"))?;
    assert_eq!(approved.scope, OAuthScope::ReadWrite);
    let redemption = CodeRedemption {
        code_hash: approval.code_hash,
        client_id: approved.client_id,
        redirect_uri: approved.redirect_uri,
        resource: approved.resource,
        code_challenge: approved.code_challenge,
    };
    let initial = token_pair(53, 54, 1_180, 1_500);
    let reversed_with_duplicate = OAuthScope::parse("crumbs:write crumbs:read crumbs:write")
        .ok_or_else(|| io::Error::other("reversed duplicate readwrite scope was rejected"))?;
    let session = database
        .redeem_code(
            &redemption,
            Some(&reversed_with_duplicate),
            &initial,
            NOW + 20,
        )
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;
    assert_eq!(session.scope, OAuthScope::ReadWrite);
    assert_eq!(session.scope.as_str(), "crumbs:read crumbs:write");

    let opposite_order = OAuthScope::parse("crumbs:write crumbs:read")
        .ok_or_else(|| io::Error::other("reversed readwrite scope was rejected"))?;
    let rotated = token_pair(55, 56, 1_800, 9_999);
    let rotated_session = database
        .rotate_refresh(
            &initial.refresh_hash,
            &client.id,
            &session.resource,
            Some(&opposite_order),
            &rotated,
            NOW + 50,
        )
        .await?
        .ok_or_else(|| io::Error::other("valid refresh token was rejected"))?;
    assert_eq!(rotated_session.scope, OAuthScope::ReadWrite);
    assert_eq!(rotated_session.scope.as_str(), "crumbs:read crumbs:write");
    let stored_scope: String = sqlx::query_scalar("SELECT scope FROM users_sessions WHERE id = ?")
        .bind(session.id)
        .fetch_one(&database.pool)
        .await?;
    assert_eq!(stored_scope, "crumbs:read crumbs:write");

    database.close().await;
    Ok(())
}

#[tokio::test]
async fn refresh_rotation_clamps_expiry_and_replay_revokes_the_family()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("refresh-client");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;
    let (_, _, redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(40),
            csrf_hash: hash(41),
            code_hash: hash(42),
            now: NOW,
            code_expires_at: 1_100,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    let initial = token_pair(43, 44, 1_150, 1_500);
    let session = database
        .redeem_code(&redemption, None, &initial, NOW + 20)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;

    let rotated = token_pair(45, 46, 1_800, 9_999);
    assert!(
        database
            .rotate_refresh(
                &initial.refresh_hash,
                "wrong-client",
                &session.resource,
                None,
                &rotated,
                1_050
            )
            .await?
            .is_none()
    );
    assert!(
        database
            .rotate_refresh(
                &initial.refresh_hash,
                &client.id,
                "wrong-resource",
                None,
                &rotated,
                1_050
            )
            .await?
            .is_none()
    );
    let extra_scope = OAuthScope::Write;
    assert!(matches!(
        database
            .rotate_refresh(
                &initial.refresh_hash,
                &client.id,
                &session.resource,
                Some(&extra_scope),
                &rotated,
                1_050,
            )
            .await,
        Err(AuthStoreError::InvalidScope)
    ));
    let approved_scope = OAuthScope::Read;
    let rotated_session = database
        .rotate_refresh(
            &initial.refresh_hash,
            &client.id,
            &session.resource,
            Some(&approved_scope),
            &rotated,
            1_050,
        )
        .await?
        .ok_or_else(|| io::Error::other("valid refresh token was rejected"))?;
    assert_eq!(rotated_session.expires_at, session.expires_at);
    let access_expiry: AuthTimestamp =
        sqlx::query_scalar("SELECT expires_at FROM oauth_tokens WHERE hash = ?")
            .bind(rotated.access_hash.as_bytes().as_slice())
            .fetch_one(&database.pool)
            .await?;
    let refresh_expiry: AuthTimestamp =
        sqlx::query_scalar("SELECT expires_at FROM oauth_tokens WHERE hash = ?")
            .bind(rotated.refresh_hash.as_bytes().as_slice())
            .fetch_one(&database.pool)
            .await?;
    assert_eq!(access_expiry, session.expires_at);
    assert_eq!(refresh_expiry, session.expires_at);
    assert!(
        database
            .authenticate_access(&initial.access_hash, &session.resource, 1_060)
            .await?
            .is_some()
    );

    let replay_tokens = token_pair(47, 48, 1_200, 0);
    assert!(
        database
            .rotate_refresh(
                &initial.refresh_hash,
                &client.id,
                &session.resource,
                None,
                &replay_tokens,
                1_100,
            )
            .await?
            .is_none()
    );
    assert!(
        database
            .authenticate_access(&rotated.access_hash, &session.resource, 1_101)
            .await?
            .is_none()
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn password_change_deactivation_and_delete_revoke_authorizations_and_sessions()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("revocation-client");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;

    let (_, _, pending_redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(60),
            csrf_hash: hash(61),
            code_hash: hash(62),
            now: NOW,
            code_expires_at: 1_500,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    database.set_user_password(user_id, &password_hash).await?;
    assert!(
        database
            .redeem_code(
                &pending_redemption,
                None,
                &token_pair(63, 64, 1_400, 1_600),
                1_100,
            )
            .await?
            .is_none()
    );

    let (_, _, first_redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(65),
            csrf_hash: hash(66),
            code_hash: hash(67),
            now: 1_100,
            code_expires_at: 1_600,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    let first_tokens = token_pair(68, 69, 1_500, 1_800);
    let first_session = database
        .redeem_code(&first_redemption, None, &first_tokens, 1_120)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;
    database.set_user_active(user_id, false).await?;
    assert!(
        database
            .authenticate_access(&first_tokens.access_hash, &first_session.resource, 1_130)
            .await?
            .is_none()
    );

    database.set_user_active(user_id, true).await?;
    let (_, _, second_redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(70),
            csrf_hash: hash(71),
            code_hash: hash(72),
            now: 1_200,
            code_expires_at: 1_700,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    let second_tokens = token_pair(73, 74, 1_600, 1_900);
    let second_session = database
        .redeem_code(&second_redemption, None, &second_tokens, 1_220)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;
    database.delete_user(user_id).await?;
    assert!(
        database
            .authenticate_access(&second_tokens.access_hash, &second_session.resource, 1_230)
            .await?
            .is_none()
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_redemptions_have_one_successful_result() -> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("concurrent-client");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;
    let (_, _, redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(80),
            csrf_hash: hash(81),
            code_hash: hash(82),
            now: NOW,
            code_expires_at: 1_200,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    let tokens = token_pair(83, 84, 1_300, 1_500);
    let (first, second) = tokio::join!(
        database.redeem_code(&redemption, None, &tokens, 1_100),
        database.redeem_code(&redemption, None, &tokens, 1_100),
    );
    let first = first?;
    let second = second?;
    assert_ne!(first.is_some(), second.is_some());
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn used_authorization_codes_survive_pruning_until_their_session_expires()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("prune-client");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;
    let (_, _, redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(90),
            csrf_hash: hash(91),
            code_hash: hash(92),
            now: NOW,
            code_expires_at: 1_100,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    let tokens = token_pair(93, 94, 1_200, 1_500);
    let session = database
        .redeem_code(&redemption, None, &tokens, 1_050)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;
    database.prune_oauth(1_300).await?;
    assert!(
        database
            .authenticate_access(&tokens.access_hash, &session.resource, 1_300)
            .await?
            .is_none()
    );

    let replay_tokens = token_pair(95, 96, 1_400, 1_600);
    assert!(
        database
            .redeem_code(&redemption, None, &replay_tokens, 1_300)
            .await?
            .is_none()
    );
    assert!(
        database
            .authenticate_access(&tokens.access_hash, &session.resource, 1_100)
            .await?
            .is_none()
    );
    database.close().await;
    Ok(())
}

#[tokio::test]
async fn database_reopen_preserves_oauth_clients_and_sessions() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crumbs.db");
    let config = DatabaseConfig { path: path.clone() };
    let database = Database::open(config.clone()).await?;
    let client = public_client("persistent-client");
    database.register_client(&client, NOW).await?;
    let (user_id, password_hash) = create_user(&database).await?;
    let (_, _, redemption) = begin_authorization(
        &database,
        AuthorizationSetup {
            client_id: &client.id,
            user_id,
            password_hash: &password_hash,
            request_hash: hash(100),
            csrf_hash: hash(101),
            code_hash: hash(102),
            now: NOW,
            code_expires_at: 1_200,
            pending_expires_at: 2_000,
        },
    )
    .await?;
    let tokens = token_pair(103, 104, 1_300, 1_500);
    let session = database
        .redeem_code(&redemption, None, &tokens, 1_100)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization code was rejected"))?;
    database.close().await;
    drop(database);

    let reopened = Database::open(config).await?;
    assert!(reopened.oauth_client(&client.id).await?.is_some());
    let persisted = reopened
        .authenticate_access(&tokens.access_hash, &session.resource, 1_200)
        .await?
        .ok_or_else(|| io::Error::other("persisted access token was not found"))?;
    assert_eq!(persisted.id, session.id);
    assert_eq!(persisted.user_id, user_id);
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn denial_requires_csrf_and_consumes_only_the_matching_request()
-> Result<(), Box<dyn StdError>> {
    let (_directory, database) = open_database().await?;
    let client = public_client("denial-client");
    database.register_client(&client, NOW).await?;
    let authorization = pending(hash(110), hash(111), &client.id, NOW, 1_500);
    database.begin_authorization(&authorization, NOW).await?;
    assert!(
        database
            .deny_authorization(&authorization.request_hash, &hash(112), NOW + 1)
            .await?
            .is_none()
    );
    assert!(
        database
            .pending_authorization(&authorization.request_hash, NOW + 1)
            .await?
            .is_some()
    );
    assert!(
        database
            .deny_authorization(
                &authorization.request_hash,
                &authorization.csrf_hash,
                NOW + 1,
            )
            .await?
            .is_some()
    );
    assert!(
        database
            .pending_authorization(&authorization.request_hash, NOW + 1)
            .await?
            .is_none()
    );
    database.close().await;
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

async fn create_user(database: &Database) -> Result<(UserId, PasswordHash), Box<dyn StdError>> {
    let password_hash = PasswordHash::parse(PASSWORD_HASH)?;
    let username = Username::new("TestUser")?;
    let user = database.create_user(&username, &password_hash).await?;
    Ok((user.id, password_hash))
}

struct AuthorizationSetup<'a> {
    client_id: &'a str,
    user_id: UserId,
    password_hash: &'a PasswordHash,
    request_hash: CredentialHash,
    csrf_hash: CredentialHash,
    code_hash: CredentialHash,
    now: AuthTimestamp,
    code_expires_at: AuthTimestamp,
    pending_expires_at: AuthTimestamp,
}

async fn begin_authorization(
    database: &Database,
    setup: AuthorizationSetup<'_>,
) -> Result<(PendingAuthorization, Approval, CodeRedemption), Box<dyn StdError>> {
    let authorization = pending(
        setup.request_hash,
        setup.csrf_hash,
        setup.client_id,
        setup.now,
        setup.pending_expires_at,
    );
    database
        .begin_authorization(&authorization, setup.now)
        .await?;
    let approval = Approval {
        request_hash: setup.request_hash,
        csrf_hash: setup.csrf_hash,
        user_id: setup.user_id,
        password_hash: setup.password_hash.clone(),
        code_hash: setup.code_hash,
        code_expires_at: setup.code_expires_at,
    };
    let approved = database
        .approve_authorization(&approval, setup.now + 1)
        .await?
        .ok_or_else(|| io::Error::other("valid authorization request was rejected"))?;
    let redemption = CodeRedemption {
        code_hash: setup.code_hash,
        client_id: approved.client_id,
        redirect_uri: approved.redirect_uri,
        resource: approved.resource,
        code_challenge: approved.code_challenge,
    };
    Ok((authorization, approval, redemption))
}

fn pending(
    request_hash: CredentialHash,
    csrf_hash: CredentialHash,
    client_id: &str,
    now: AuthTimestamp,
    expires_at: AuthTimestamp,
) -> PendingAuthorization {
    PendingAuthorization {
        request_hash,
        csrf_hash,
        grant: AuthorizationGrant {
            client_id: client_id.to_owned(),
            redirect_uri: "https://client.example/callback".to_owned(),
            resource: "https://api.example".to_owned(),
            scope: OAuthScope::Read,
            state: Some("client-state".to_owned()),
            code_challenge: "challenge-123".to_owned(),
        },
        expires_at: expires_at.max(now + 1),
    }
}

fn public_client(id: &str) -> OAuthClient {
    OAuthClient {
        id: id.to_owned(),
        name: format!("{id} client"),
        redirect_uris: vec!["https://client.example/callback".to_owned()],
        authentication: ClientAuthentication::Public,
        allow_refresh: true,
    }
}

fn token_pair(
    access_hash: u8,
    refresh_hash: u8,
    access_expires_at: AuthTimestamp,
    session_expires_at: AuthTimestamp,
) -> TokenPair {
    TokenPair {
        access_hash: hash(access_hash),
        refresh_hash: hash(refresh_hash),
        access_expires_at,
        session_expires_at,
    }
}

fn hash(value: u8) -> CredentialHash {
    CredentialHash::new([value; 32])
}
