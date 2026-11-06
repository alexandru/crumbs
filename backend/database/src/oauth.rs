//! Persistent OAuth clients, authorizations, sessions, and token hashes.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use crate::users::{PasswordHash, UserId};

mod sqlite;

/// SHA-256 digest of an opaque OAuth credential.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CredentialHash([u8; 32]);

impl CredentialHash {
    /// Creates a credential digest from its fixed-size SHA-256 bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for CredentialHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialHash([REDACTED])")
    }
}

/// Registered OAuth client identifier.
pub type ClientId = String;
/// Display name of a registered OAuth client.
pub type ClientName = String;
/// Client redirect URI registered for authorization.
pub type RedirectUri = String;
/// Resource server URI associated with a grant or session.
pub type ResourceUri = String;
/// Granted OAuth scopes.
pub type OAuthScope = String;
/// Client-provided OAuth state value.
pub type ClientState = String;
/// PKCE challenge value.
pub type PkceChallenge = String;
/// Local database identifier for a user session.
pub type SessionId = i64;
/// Caller-supplied timestamp in milliseconds since the Unix epoch.
pub type AuthTimestamp = i64;

/// Authentication method configured for an OAuth client.
#[derive(Clone, Debug)]
pub enum ClientAuthentication {
    /// The client has no shared secret.
    Public,
    /// The client authenticates with an HTTP Basic secret.
    SecretBasic(CredentialHash),
    /// The client authenticates with a form-post secret.
    SecretPost(CredentialHash),
}

/// A registered OAuth client and its allowed redirect URIs.
#[derive(Clone, Debug)]
pub struct OAuthClient {
    /// Registered client identifier.
    pub id: ClientId,
    /// Human-readable client name.
    pub name: ClientName,
    /// Redirect URIs allowed for authorization.
    pub redirect_uris: Vec<RedirectUri>,
    /// Client authentication method and hashed secret, if applicable.
    pub authentication: ClientAuthentication,
    /// Whether the server may issue refresh credentials to this client.
    pub allow_refresh: bool,
}

/// Authorization details retained while a user approves a client request.
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

/// Authorization request indexed by its request and CSRF credential hashes.
#[derive(Clone, Debug)]
pub struct PendingAuthorization {
    pub request_hash: CredentialHash,
    pub csrf_hash: CredentialHash,
    pub grant: AuthorizationGrant,
    pub expires_at: AuthTimestamp,
}

/// Approval command guarded by the current user's password hash.
#[derive(Clone, Debug)]
pub struct Approval {
    pub request_hash: CredentialHash,
    pub csrf_hash: CredentialHash,
    pub user_id: UserId,
    pub password_hash: PasswordHash,
    pub code_hash: CredentialHash,
    pub code_expires_at: AuthTimestamp,
}

/// One-time authorization-code redemption request.
#[derive(Clone, Debug)]
pub struct CodeRedemption {
    pub code_hash: CredentialHash,
    pub client_id: ClientId,
    pub redirect_uri: RedirectUri,
    pub resource: ResourceUri,
    pub code_challenge: PkceChallenge,
}

/// Hashes and expiries for a newly issued access and refresh token pair.
#[derive(Clone, Copy, Debug)]
pub struct TokenPair {
    pub access_hash: CredentialHash,
    pub refresh_hash: CredentialHash,
    pub access_expires_at: AuthTimestamp,
    pub session_expires_at: AuthTimestamp,
}

/// Public metadata for an authenticated user session.
#[derive(Clone, Debug)]
pub struct UserSession {
    pub id: SessionId,
    pub user_id: UserId,
    pub client_id: ClientId,
    pub resource: ResourceUri,
    pub scope: OAuthScope,
    pub expires_at: AuthTimestamp,
}

/// Errors produced by the OAuth persistence API.
#[derive(Debug, thiserror::Error)]
pub enum AuthStoreError {
    /// The storage operation failed.
    #[error("OAuth storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    /// A persistence record violates its declared invariants.
    #[error("invalid OAuth record")]
    InvalidRecord,
    /// The OAuth client or pending authorization capacity has been reached.
    #[error("OAuth storage capacity reached")]
    Capacity,
    /// The requested scope does not match the approved scope.
    #[error("OAuth scope does not match the approved grant")]
    InvalidScope,
}

/// Persists OAuth state without exposing storage-engine types.
pub trait OAuthStore: Sync {
    /// Registers a new OAuth client. An existing identifier returns
    /// [`AuthStoreError::InvalidRecord`].
    fn register_client(
        &self,
        client: &OAuthClient,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    /// Looks up a registered OAuth client.
    fn oauth_client(
        &self,
        client_id: &str,
    ) -> impl Future<Output = Result<Option<OAuthClient>, AuthStoreError>> + Send;

    /// Stores a pending authorization request if capacity permits.
    fn begin_authorization(
        &self,
        authorization: &PendingAuthorization,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    /// Finds an unexpired, pending authorization request.
    fn pending_authorization(
        &self,
        request_hash: &CredentialHash,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<PendingAuthorization>, AuthStoreError>> + Send;

    /// Atomically approves a pending request when its CSRF and user credentials match.
    fn approve_authorization(
        &self,
        approval: &Approval,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<AuthorizationGrant>, AuthStoreError>> + Send;

    /// Consumes a pending request after its CSRF hash matches.
    fn deny_authorization(
        &self,
        request_hash: &CredentialHash,
        csrf_hash: &CredentialHash,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<AuthorizationGrant>, AuthStoreError>> + Send;

    /// Redeems a matching, unexpired authorization code and creates a user
    /// session and token pair. When `requested_scope` is present, it must match
    /// the approved scope. Invalid, expired, mismatched, or wrong-scope codes
    /// return without consuming an unused code. Reusing a matching redeemed
    /// code revokes its session family and commits before returning `None`,
    /// even when the code has since expired.
    fn redeem_code(
        &self,
        redemption: &CodeRedemption,
        requested_scope: Option<&str>,
        tokens: &TokenPair,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<UserSession>, AuthStoreError>> + Send;

    /// Rotates a live refresh token. When `requested_scope` is present, it must
    /// match the session's approved scope. A mismatch does not consume the
    /// refresh token. Reusing a matching used refresh token revokes its session
    /// family and commits before returning `None`. Rotation keeps the session's
    /// original expiry and clamps the new access-token expiry to that absolute
    /// time.
    fn rotate_refresh(
        &self,
        refresh_hash: &CredentialHash,
        client_id: &str,
        resource: &str,
        requested_scope: Option<&str>,
        tokens: &TokenPair,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<UserSession>, AuthStoreError>> + Send;

    /// Authenticates an access token for the supplied resource.
    fn authenticate_access(
        &self,
        access_hash: &CredentialHash,
        resource: &str,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<Option<UserSession>, AuthStoreError>> + Send;

    /// Revokes the session family for a token owned by the client.
    fn revoke_token(
        &self,
        token_hash: &CredentialHash,
        client_id: &str,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    /// Revokes a user's sessions and outstanding authorization requests.
    fn revoke_user_sessions(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;

    /// Removes expired OAuth data and old unused clients.
    fn prune_oauth(
        &self,
        now: AuthTimestamp,
    ) -> impl Future<Output = Result<(), AuthStoreError>> + Send;
}

#[cfg(test)]
#[path = "../unit-tests/oauth.rs"]
mod tests;
