use sqlx::{FromRow, Sqlite, Transaction};

use super::{
    Approval, AuthStoreError, AuthTimestamp, AuthorizationGrant, ClientAuthentication,
    CodeRedemption, CredentialHash, OAuthClient, OAuthScope, OAuthStore, PendingAuthorization,
    ResourceUri, SessionId, TokenPair, UserSession,
};
use crate::Database;
use crate::users::UserId;

impl OAuthStore for Database {
    async fn register_client(
        &self,
        client: &OAuthClient,
        now: AuthTimestamp,
    ) -> Result<(), AuthStoreError> {
        if client.id.is_empty() || client.name.is_empty() {
            return Err(AuthStoreError::InvalidRecord);
        }
        let redirect_uris = serde_json::to_string(&client.redirect_uris)
            .map_err(|source| AuthStoreError::Storage(Box::new(source)))?;
        let (authentication, secret_hash) = client_authentication_parts(&client.authentication);
        let mut transaction = begin_immediate(self).await?;
        let existing: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_clients WHERE id = ?)")
                .bind(&client.id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(storage_error)?;
        if existing {
            return Err(AuthStoreError::InvalidRecord);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oauth_clients")
            .fetch_one(&mut *transaction)
            .await
            .map_err(storage_error)?;
        if count >= MAX_OAUTH_CLIENTS {
            return Err(AuthStoreError::Capacity);
        }
        sqlx::query(
            "INSERT INTO oauth_clients
                (id, name, redirect_uris, authentication, secret_hash, allow_refresh, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&client.id)
        .bind(&client.name)
        .bind(redirect_uris)
        .bind(authentication)
        .bind(secret_hash.map(|hash| hash.as_bytes().as_slice()))
        .bind(if client.allow_refresh { 1_i64 } else { 0_i64 })
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(map_write_error)?;
        transaction.commit().await.map_err(storage_error)
    }

    async fn oauth_client(&self, client_id: &str) -> Result<Option<OAuthClient>, AuthStoreError> {
        let row = sqlx::query_as::<_, OAuthClientRow>(
            "SELECT id, name, redirect_uris, authentication, secret_hash, allow_refresh
             FROM oauth_clients WHERE id = ?",
        )
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(OAuthClientRow::into_client).transpose()
    }

    async fn begin_authorization(
        &self,
        authorization: &PendingAuthorization,
        now: AuthTimestamp,
    ) -> Result<(), AuthStoreError> {
        if authorization.expires_at <= now {
            return Err(AuthStoreError::InvalidRecord);
        }
        let mut transaction = begin_immediate(self).await?;
        let existing: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oauth_authorizations WHERE request_hash = ?)",
        )
        .bind(authorization.request_hash.as_bytes().as_slice())
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if existing {
            return Err(AuthStoreError::InvalidRecord);
        }
        sqlx::query(
            "DELETE FROM oauth_authorizations
             WHERE code_hash IS NULL AND expires_at <= ?",
        )
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let pending_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM oauth_authorizations
             WHERE code_hash IS NULL AND expires_at > ?",
        )
        .bind(now)
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if pending_count >= MAX_PENDING_AUTHORIZATIONS {
            transaction.commit().await.map_err(storage_error)?;
            return Err(AuthStoreError::Capacity);
        }
        sqlx::query(
            "INSERT INTO oauth_authorizations (
                request_hash, csrf_hash, client_id, redirect_uri, resource, scope,
                state, code_challenge, expires_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(authorization.request_hash.as_bytes().as_slice())
        .bind(authorization.csrf_hash.as_bytes().as_slice())
        .bind(&authorization.grant.client_id)
        .bind(&authorization.grant.redirect_uri)
        .bind(&authorization.grant.resource)
        .bind(&authorization.grant.scope)
        .bind(&authorization.grant.state)
        .bind(&authorization.grant.code_challenge)
        .bind(authorization.expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(map_write_error)?;
        transaction.commit().await.map_err(storage_error)
    }

    async fn pending_authorization(
        &self,
        request_hash: &CredentialHash,
        now: AuthTimestamp,
    ) -> Result<Option<PendingAuthorization>, AuthStoreError> {
        let row = sqlx::query_as::<_, AuthorizationRow>(
            "SELECT request_hash, csrf_hash, client_id, redirect_uri, resource, scope,
                    state, code_challenge, expires_at, user_id, code_hash, session_id
             FROM oauth_authorizations
             WHERE request_hash = ? AND code_hash IS NULL AND expires_at > ?",
        )
        .bind(request_hash.as_bytes().as_slice())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(AuthorizationRow::into_pending).transpose()
    }

    async fn approve_authorization(
        &self,
        approval: &Approval,
        now: AuthTimestamp,
    ) -> Result<Option<AuthorizationGrant>, AuthStoreError> {
        if approval.code_expires_at <= now {
            return Err(AuthStoreError::InvalidRecord);
        }
        let mut transaction = begin_immediate(self).await?;
        let row = sqlx::query_as::<_, AuthorizationRow>(
            "SELECT request_hash, csrf_hash, client_id, redirect_uri, resource, scope,
                    state, code_challenge, expires_at, user_id, code_hash, session_id
             FROM oauth_authorizations
             WHERE request_hash = ? AND code_hash IS NULL AND expires_at > ?",
        )
        .bind(approval.request_hash.as_bytes().as_slice())
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        };
        if row.csrf_hash.as_slice() != approval.csrf_hash.as_bytes() {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        if row.user_id.is_some() || row.code_hash.is_some() || row.session_id.is_some() {
            return Err(AuthStoreError::InvalidRecord);
        }
        let user_is_current_and_active: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM users
                WHERE id = ? AND active = 1 AND password_hash = ?
             )",
        )
        .bind(approval.user_id.get())
        .bind(approval.password_hash.as_str())
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if !user_is_current_and_active {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        let updated = sqlx::query(
            "UPDATE oauth_authorizations
             SET user_id = ?, code_hash = ?, expires_at = ?
             WHERE request_hash = ? AND csrf_hash = ? AND code_hash IS NULL AND expires_at > ?",
        )
        .bind(approval.user_id.get())
        .bind(approval.code_hash.as_bytes().as_slice())
        .bind(approval.code_expires_at)
        .bind(approval.request_hash.as_bytes().as_slice())
        .bind(approval.csrf_hash.as_bytes().as_slice())
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(map_write_error)?;
        if updated.rows_affected() == 0 {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        let grant = row.grant();
        transaction.commit().await.map_err(storage_error)?;
        Ok(Some(grant))
    }

    async fn deny_authorization(
        &self,
        request_hash: &CredentialHash,
        csrf_hash: &CredentialHash,
        now: AuthTimestamp,
    ) -> Result<Option<AuthorizationGrant>, AuthStoreError> {
        let mut transaction = begin_immediate(self).await?;
        let row = sqlx::query_as::<_, AuthorizationRow>(
            "SELECT request_hash, csrf_hash, client_id, redirect_uri, resource, scope,
                    state, code_challenge, expires_at, user_id, code_hash, session_id
             FROM oauth_authorizations
             WHERE request_hash = ? AND code_hash IS NULL AND expires_at > ?",
        )
        .bind(request_hash.as_bytes().as_slice())
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        };
        if row.csrf_hash.as_slice() != csrf_hash.as_bytes() {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        if row.user_id.is_some() || row.session_id.is_some() {
            return Err(AuthStoreError::InvalidRecord);
        }
        let grant = row.grant();
        sqlx::query(
            "DELETE FROM oauth_authorizations
             WHERE request_hash = ? AND csrf_hash = ? AND code_hash IS NULL AND expires_at > ?",
        )
        .bind(request_hash.as_bytes().as_slice())
        .bind(csrf_hash.as_bytes().as_slice())
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)?;
        Ok(Some(grant))
    }

    async fn redeem_code(
        &self,
        redemption: &CodeRedemption,
        tokens: &TokenPair,
        now: AuthTimestamp,
    ) -> Result<Option<UserSession>, AuthStoreError> {
        validate_new_token_pair(tokens, now, Some(&redemption.code_hash))?;
        let mut transaction = begin_immediate(self).await?;
        let row = sqlx::query_as::<_, AuthorizationRow>(
            "SELECT request_hash, csrf_hash, client_id, redirect_uri, resource, scope,
                    state, code_challenge, expires_at, user_id, code_hash, session_id
             FROM oauth_authorizations WHERE code_hash = ?",
        )
        .bind(redemption.code_hash.as_bytes().as_slice())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        };
        if !row.matches_redemption(redemption) {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        if let Some(session_id) = row.session_id {
            sqlx::query("DELETE FROM users_sessions WHERE id = ?")
                .bind(session_id)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error)?;
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        if row.expires_at <= now {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        let Some(user_id) = row.user_id else {
            return Err(AuthStoreError::InvalidRecord);
        };
        let user_active: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = ? AND active = 1)")
                .bind(user_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(storage_error)?;
        if !user_active {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        let session_id: SessionId = sqlx::query_scalar(
            "INSERT INTO users_sessions
                (user_id, client_id, resource, scope, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING id",
        )
        .bind(user_id)
        .bind(&row.client_id)
        .bind(&row.resource)
        .bind(&row.scope)
        .bind(now)
        .bind(tokens.session_expires_at)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_write_error)?;
        insert_token(
            &mut transaction,
            tokens.access_hash,
            session_id,
            TOKEN_ACCESS,
            tokens.access_expires_at,
        )
        .await?;
        insert_token(
            &mut transaction,
            tokens.refresh_hash,
            session_id,
            TOKEN_REFRESH,
            tokens.session_expires_at,
        )
        .await?;
        let updated = sqlx::query(
            "UPDATE oauth_authorizations SET session_id = ?
             WHERE code_hash = ? AND session_id IS NULL",
        )
        .bind(session_id)
        .bind(redemption.code_hash.as_bytes().as_slice())
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if updated.rows_affected() != 1 {
            transaction.rollback().await.map_err(storage_error)?;
            return Ok(None);
        }
        let session = UserSession {
            id: session_id,
            user_id: UserId::new(user_id).map_err(|_| AuthStoreError::InvalidRecord)?,
            client_id: row.client_id,
            resource: row.resource,
            scope: row.scope,
            expires_at: tokens.session_expires_at,
        };
        transaction.commit().await.map_err(storage_error)?;
        Ok(Some(session))
    }

    async fn rotate_refresh(
        &self,
        refresh_hash: &CredentialHash,
        client_id: &str,
        resource: &str,
        tokens: &TokenPair,
        now: AuthTimestamp,
    ) -> Result<Option<UserSession>, AuthStoreError> {
        validate_refresh_token_pair(tokens, now, refresh_hash)?;
        let mut transaction = begin_immediate(self).await?;
        let row = sqlx::query_as::<_, RefreshSessionRow>(
            "SELECT t.session_id, t.expires_at AS token_expires_at, t.used,
                    s.user_id, s.client_id, s.resource, s.scope, s.expires_at AS session_expires_at,
                    u.active, c.allow_refresh
             FROM oauth_tokens AS t
             JOIN users_sessions AS s ON s.id = t.session_id
             JOIN users AS u ON u.id = s.user_id
             JOIN oauth_clients AS c ON c.id = s.client_id
             WHERE t.hash = ? AND t.kind = 'refresh' AND s.client_id = ? AND s.resource = ?",
        )
        .bind(refresh_hash.as_bytes().as_slice())
        .bind(client_id)
        .bind(resource)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        };
        if row.session_expires_at <= now
            || row.token_expires_at <= now
            || row.active != 1
            || row.allow_refresh != 1
        {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        if row.used == 1 {
            sqlx::query("DELETE FROM users_sessions WHERE id = ?")
                .bind(row.session_id)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error)?;
            transaction.commit().await.map_err(storage_error)?;
            return Ok(None);
        }
        if row.used != 0 {
            return Err(AuthStoreError::InvalidRecord);
        }
        let access_expires_at = tokens.access_expires_at.min(row.session_expires_at);
        if access_expires_at <= now {
            return Err(AuthStoreError::InvalidRecord);
        }
        let used = sqlx::query("UPDATE oauth_tokens SET used = 1 WHERE hash = ? AND used = 0")
            .bind(refresh_hash.as_bytes().as_slice())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        if used.rows_affected() != 1 {
            transaction.rollback().await.map_err(storage_error)?;
            return Ok(None);
        }
        insert_token(
            &mut transaction,
            tokens.access_hash,
            row.session_id,
            TOKEN_ACCESS,
            access_expires_at,
        )
        .await?;
        insert_token(
            &mut transaction,
            tokens.refresh_hash,
            row.session_id,
            TOKEN_REFRESH,
            row.session_expires_at,
        )
        .await?;
        let session = UserSession {
            id: row.session_id,
            user_id: UserId::new(row.user_id).map_err(|_| AuthStoreError::InvalidRecord)?,
            client_id: row.client_id,
            resource: row.resource,
            scope: row.scope,
            expires_at: row.session_expires_at,
        };
        transaction.commit().await.map_err(storage_error)?;
        Ok(Some(session))
    }

    async fn authenticate_access(
        &self,
        access_hash: &CredentialHash,
        resource: &str,
        now: AuthTimestamp,
    ) -> Result<Option<UserSession>, AuthStoreError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT s.id, s.user_id, s.client_id, s.resource, s.scope, s.expires_at
             FROM oauth_tokens AS t
             JOIN users_sessions AS s ON s.id = t.session_id
             JOIN users AS u ON u.id = s.user_id
             WHERE t.hash = ? AND t.kind = 'access' AND t.expires_at > ?
               AND s.expires_at > ? AND s.resource = ? AND u.active = 1",
        )
        .bind(access_hash.as_bytes().as_slice())
        .bind(now)
        .bind(now)
        .bind(resource)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(SessionRow::into_session).transpose()
    }

    async fn revoke_token(
        &self,
        token_hash: &CredentialHash,
        client_id: &str,
    ) -> Result<(), AuthStoreError> {
        sqlx::query(
            "DELETE FROM users_sessions
             WHERE id IN (
                SELECT t.session_id FROM oauth_tokens AS t
                JOIN users_sessions AS s ON s.id = t.session_id
                WHERE t.hash = ? AND s.client_id = ?
             )",
        )
        .bind(token_hash.as_bytes().as_slice())
        .bind(client_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn revoke_user_sessions(&self, user_id: UserId) -> Result<(), AuthStoreError> {
        let mut transaction = begin_immediate(self).await?;
        sqlx::query("DELETE FROM users_sessions WHERE user_id = ?")
            .bind(user_id.get())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        sqlx::query("DELETE FROM oauth_authorizations WHERE user_id = ?")
            .bind(user_id.get())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)
    }

    async fn prune_oauth(&self, now: AuthTimestamp) -> Result<(), AuthStoreError> {
        let mut transaction = begin_immediate(self).await?;
        sqlx::query("DELETE FROM users_sessions WHERE expires_at <= ?")
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        sqlx::query(
            "DELETE FROM oauth_authorizations
             WHERE expires_at <= ? AND session_id IS NULL",
        )
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        sqlx::query("DELETE FROM oauth_tokens WHERE kind = 'access' AND expires_at <= ?")
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        let clients_older_than = now.saturating_sub(CLIENT_RETENTION_MILLIS);
        sqlx::query(
            "DELETE FROM oauth_clients AS c
             WHERE c.created_at <= ?
               AND NOT EXISTS (SELECT 1 FROM oauth_authorizations AS a WHERE a.client_id = c.id)
               AND NOT EXISTS (SELECT 1 FROM users_sessions AS s WHERE s.client_id = c.id)",
        )
        .bind(clients_older_than)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)
    }
}

async fn begin_immediate(
    database: &Database,
) -> Result<Transaction<'static, Sqlite>, AuthStoreError> {
    database
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(storage_error)
}

async fn insert_token(
    transaction: &mut Transaction<'_, Sqlite>,
    hash: CredentialHash,
    session_id: SessionId,
    kind: &str,
    expires_at: AuthTimestamp,
) -> Result<(), AuthStoreError> {
    sqlx::query(
        "INSERT INTO oauth_tokens (hash, session_id, kind, expires_at, used)
         VALUES (?, ?, ?, ?, 0)",
    )
    .bind(hash.as_bytes().as_slice())
    .bind(session_id)
    .bind(kind)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_write_error)?;
    Ok(())
}

fn validate_new_token_pair(
    tokens: &TokenPair,
    now: AuthTimestamp,
    other_hash: Option<&CredentialHash>,
) -> Result<(), AuthStoreError> {
    if tokens.access_hash == tokens.refresh_hash
        || other_hash
            .is_some_and(|hash| tokens.access_hash == *hash || tokens.refresh_hash == *hash)
        || tokens.access_expires_at <= now
        || tokens.session_expires_at <= now
        || tokens.access_expires_at > tokens.session_expires_at
    {
        return Err(AuthStoreError::InvalidRecord);
    }
    Ok(())
}

fn validate_refresh_token_pair(
    tokens: &TokenPair,
    now: AuthTimestamp,
    previous_refresh_hash: &CredentialHash,
) -> Result<(), AuthStoreError> {
    if tokens.access_hash == tokens.refresh_hash
        || tokens.access_hash == *previous_refresh_hash
        || tokens.refresh_hash == *previous_refresh_hash
        || tokens.access_expires_at <= now
    {
        return Err(AuthStoreError::InvalidRecord);
    }
    Ok(())
}

fn client_authentication_parts(
    authentication: &ClientAuthentication,
) -> (&'static str, Option<&CredentialHash>) {
    match authentication {
        ClientAuthentication::Public => (CLIENT_PUBLIC, None),
        ClientAuthentication::SecretBasic(hash) => (CLIENT_SECRET_BASIC, Some(hash)),
        ClientAuthentication::SecretPost(hash) => (CLIENT_SECRET_POST, Some(hash)),
    }
}

fn storage_error(source: sqlx::Error) -> AuthStoreError {
    AuthStoreError::Storage(Box::new(source))
}

fn map_write_error(source: sqlx::Error) -> AuthStoreError {
    if source.as_database_error().is_some_and(|database| {
        database.is_unique_violation()
            || database.is_foreign_key_violation()
            || database.is_check_violation()
    }) {
        AuthStoreError::InvalidRecord
    } else {
        storage_error(source)
    }
}

fn decode_hash(bytes: Vec<u8>) -> Result<CredentialHash, AuthStoreError> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AuthStoreError::InvalidRecord)?;
    Ok(CredentialHash::new(bytes))
}

#[derive(FromRow)]
struct OAuthClientRow {
    id: String,
    name: String,
    redirect_uris: String,
    authentication: String,
    secret_hash: Option<Vec<u8>>,
    allow_refresh: i64,
}

impl OAuthClientRow {
    fn into_client(self) -> Result<OAuthClient, AuthStoreError> {
        let redirect_uris =
            serde_json::from_str(&self.redirect_uris).map_err(|_| AuthStoreError::InvalidRecord)?;
        let authentication = match (self.authentication.as_str(), self.secret_hash) {
            (CLIENT_PUBLIC, None) => ClientAuthentication::Public,
            (CLIENT_SECRET_BASIC, Some(hash)) => {
                ClientAuthentication::SecretBasic(decode_hash(hash)?)
            }
            (CLIENT_SECRET_POST, Some(hash)) => {
                ClientAuthentication::SecretPost(decode_hash(hash)?)
            }
            _ => return Err(AuthStoreError::InvalidRecord),
        };
        let allow_refresh = match self.allow_refresh {
            0 => false,
            1 => true,
            _ => return Err(AuthStoreError::InvalidRecord),
        };
        Ok(OAuthClient {
            id: self.id,
            name: self.name,
            redirect_uris,
            authentication,
            allow_refresh,
        })
    }
}

#[derive(FromRow)]
struct AuthorizationRow {
    request_hash: Vec<u8>,
    csrf_hash: Vec<u8>,
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    state: Option<String>,
    code_challenge: String,
    expires_at: AuthTimestamp,
    user_id: Option<i64>,
    code_hash: Option<Vec<u8>>,
    session_id: Option<SessionId>,
}

impl AuthorizationRow {
    fn grant(&self) -> AuthorizationGrant {
        AuthorizationGrant {
            client_id: self.client_id.clone(),
            redirect_uri: self.redirect_uri.clone(),
            resource: self.resource.clone(),
            scope: self.scope.clone(),
            state: self.state.clone(),
            code_challenge: self.code_challenge.clone(),
        }
    }

    fn into_pending(self) -> Result<PendingAuthorization, AuthStoreError> {
        if self.user_id.is_some() || self.code_hash.is_some() || self.session_id.is_some() {
            return Err(AuthStoreError::InvalidRecord);
        }
        let grant = self.grant();
        Ok(PendingAuthorization {
            request_hash: decode_hash(self.request_hash)?,
            csrf_hash: decode_hash(self.csrf_hash)?,
            grant,
            expires_at: self.expires_at,
        })
    }

    fn matches_redemption(&self, redemption: &CodeRedemption) -> bool {
        self.client_id == redemption.client_id
            && self.redirect_uri == redemption.redirect_uri
            && self.resource == redemption.resource
            && self.code_challenge == redemption.code_challenge
    }
}

#[derive(FromRow)]
struct SessionRow {
    id: SessionId,
    user_id: i64,
    client_id: String,
    resource: ResourceUri,
    scope: OAuthScope,
    expires_at: AuthTimestamp,
}

impl SessionRow {
    fn into_session(self) -> Result<UserSession, AuthStoreError> {
        if self.id <= 0 {
            return Err(AuthStoreError::InvalidRecord);
        }
        Ok(UserSession {
            id: self.id,
            user_id: UserId::new(self.user_id).map_err(|_| AuthStoreError::InvalidRecord)?,
            client_id: self.client_id,
            resource: self.resource,
            scope: self.scope,
            expires_at: self.expires_at,
        })
    }
}

#[derive(FromRow)]
struct RefreshSessionRow {
    session_id: SessionId,
    token_expires_at: AuthTimestamp,
    used: i64,
    user_id: i64,
    client_id: String,
    resource: ResourceUri,
    scope: OAuthScope,
    session_expires_at: AuthTimestamp,
    active: i64,
    allow_refresh: i64,
}

const MAX_OAUTH_CLIENTS: i64 = 4_096;
const MAX_PENDING_AUTHORIZATIONS: i64 = 4_096;
const CLIENT_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const TOKEN_ACCESS: &str = "access";
const TOKEN_REFRESH: &str = "refresh";
const CLIENT_PUBLIC: &str = "public";
const CLIENT_SECRET_BASIC: &str = "secret_basic";
const CLIENT_SECRET_POST: &str = "secret_post";

#[cfg(test)]
#[path = "../../unit-tests/oauth/sqlite.rs"]
mod tests;
