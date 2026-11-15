use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use super::users::{PasswordHash, UserId};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CredentialHash([u8; 32]);

impl CredentialHash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for CredentialHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialHash([REDACTED])")
    }
}

pub type ClientId = String;
pub type ClientName = String;
pub type RedirectUri = String;
pub type ResourceUri = String;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OAuthScope {
    Read,
    Write,
    ReadWrite,
}

impl OAuthScope {
    pub fn parse(value: &str) -> Option<Self> {
        let mut read = false;
        let mut write = false;

        for scope in value.split_whitespace() {
            match scope {
                "crumbs:read" => read = true,
                "crumbs:write" => write = true,
                _ => return None,
            }
        }

        match (read, write) {
            (true, false) => Some(Self::Read),
            (false, true) => Some(Self::Write),
            (true, true) => Some(Self::ReadWrite),
            (false, false) => None,
        }
    }

    pub const fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub const fn can_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }

    pub fn includes(self, required: Self) -> bool {
        matches!(self, Self::ReadWrite) || self == required
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "crumbs:read",
            Self::Write => "crumbs:write",
            Self::ReadWrite => "crumbs:read crumbs:write",
        }
    }
}

impl fmt::Display for OAuthScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
pub type ClientState = String;
pub type PkceChallenge = String;
pub type SessionId = i64;
pub type AuthTimestamp = i64;

#[derive(Clone, Debug)]
pub enum ClientAuthentication {
    Public,
    SecretBasic(CredentialHash),
    SecretPost(CredentialHash),
}

#[derive(Clone, Debug)]
pub struct OAuthClient {
    pub id: ClientId,
    pub name: ClientName,
    pub redirect_uris: Vec<RedirectUri>,
    pub authentication: ClientAuthentication,
    pub allow_refresh: bool,
}

#[derive(Clone)]
pub struct AuthorizationGrant {
    pub client_id: ClientId,
    pub redirect_uri: RedirectUri,
    pub resource: ResourceUri,
    pub scope: OAuthScope,
    pub state: Option<ClientState>,
    pub code_challenge: PkceChallenge,
}

impl fmt::Debug for AuthorizationGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationGrant")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("resource", &self.resource)
            .field("scope", &self.scope)
            .field("state", &self.state.as_ref().map(|_| "[REDACTED]"))
            .field("code_challenge", &self.code_challenge)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct PendingAuthorization {
    pub request_hash: CredentialHash,
    pub csrf_hash: CredentialHash,
    pub grant: AuthorizationGrant,
    pub expires_at: AuthTimestamp,
}

#[derive(Clone, Debug)]
pub struct Approval {
    pub request_hash: CredentialHash,
    pub csrf_hash: CredentialHash,
    pub user_id: UserId,
    pub password_hash: PasswordHash,
    pub code_hash: CredentialHash,
    pub code_expires_at: AuthTimestamp,
}

#[derive(Clone, Debug)]
pub struct CodeRedemption {
    pub code_hash: CredentialHash,
    pub client_id: ClientId,
    pub redirect_uri: RedirectUri,
    pub resource: ResourceUri,
    pub code_challenge: PkceChallenge,
}

#[derive(Clone, Copy, Debug)]
pub struct TokenPair {
    pub access_hash: CredentialHash,
    pub refresh_hash: CredentialHash,
    pub access_expires_at: AuthTimestamp,
    pub session_expires_at: AuthTimestamp,
}

#[derive(Clone, Debug)]
pub struct UserSession {
    pub id: SessionId,
    pub user_id: UserId,
    pub client_id: ClientId,
    pub resource: ResourceUri,
    pub scope: OAuthScope,
    pub expires_at: AuthTimestamp,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthStoreError {
    #[error("OAuth storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error("invalid OAuth record")]
    InvalidRecord,
    #[error("OAuth storage capacity reached")]
    Capacity,
    #[error("OAuth scope does not match the approved grant")]
    InvalidScope,
}

pub trait OAuthStore: Sync {
    fn register_client(
        &self,
        client: &OAuthClient,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    fn oauth_client(
        &self,
        client_id: &str,
    ) -> impl Future<Output = Result<Option<OAuthClient>, AuthStoreError>> + Send;

    fn begin_authorization(
        &self,
        authorization: &PendingAuthorization,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    fn pending_authorization(
        &self,
        request_hash: &CredentialHash,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<PendingAuthorization>, AuthStoreError>> + Send;

    fn approve_authorization(
        &self,
        approval: &Approval,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<AuthorizationGrant>, AuthStoreError>> + Send;

    fn deny_authorization(
        &self,
        request_hash: &CredentialHash,
        csrf_hash: &CredentialHash,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<AuthorizationGrant>, AuthStoreError>> + Send;

    fn redeem_code(
        &self,
        redemption: &CodeRedemption,
        requested_scope: Option<&OAuthScope>,
        tokens: &TokenPair,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<UserSession>, AuthStoreError>> + Send;

    fn rotate_refresh(
        &self,
        refresh_hash: &CredentialHash,
        client_id: &str,
        resource: &str,
        requested_scope: Option<&OAuthScope>,
        tokens: &TokenPair,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<UserSession>, AuthStoreError>> + Send;

    fn authenticate_access(
        &self,
        access_hash: &CredentialHash,
        resource: &str,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<UserSession>, AuthStoreError>> + Send;

    fn revoke_token(
        &self,
        token_hash: &CredentialHash,
        client_id: &str,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    fn revoke_user_sessions(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    fn prune_oauth(
        &self,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;
}

#[cfg(test)]
#[path = "../../unit-tests/identity/oauth.rs"]
mod tests;
