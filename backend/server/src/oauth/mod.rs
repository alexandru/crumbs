//! OAuth discovery, authorization, and access-token enforcement.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use crumbs_database::oauth::OAuthStore;
use crumbs_database::users::{PasswordHash, UserError, Users};
use serde_json::json;
use tokio::sync::Semaphore;

use crate::mcp::McpAuthorization;

mod authorization;
mod config;
mod limits;
mod protocol;
mod routes;
mod tokens;
mod views;

pub use config::{InvalidIssuerUrl, IssuerUrl, OAuthConfig};
pub(crate) use routes::routes;

pub(crate) struct AuthorizationServer<S> {
    store: Arc<S>,
    issuer: IssuerUrl,
    dummy_password: PasswordHash,
    password_slots: Arc<Semaphore>,
    limits: limits::Limits,
}

pub(crate) use crumbs_database::oauth::OAuthScope as FoodScopes;

impl<S> AuthorizationServer<S>
where
    S: Users + OAuthStore + Send + 'static,
{
    pub(crate) async fn new(store: Arc<S>, config: OAuthConfig) -> Result<Arc<Self>, InitError> {
        let dummy_password = tokio::task::spawn_blocking(|| {
            PasswordHash::hash("Crumbs invalid-account password sentinel")
        })
        .await??;
        Ok(Arc::new(Self {
            store,
            issuer: config.issuer,
            dummy_password,
            password_slots: Arc::new(Semaphore::new(4)),
            limits: limits::Limits::default(),
        }))
    }

    pub(crate) async fn cleanup(&self) {
        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            ticks.tick().await;
            if let Ok(now) = protocol::now()
                && let Err(error) = self.store.prune_oauth(now).await
            {
                eprintln!("OAuth cleanup failed: {error}");
            }
        }
    }

    fn resource(&self, mcp: bool) -> String {
        self.issuer.endpoint(if mcp { "/mcp" } else { "/" })
    }

    fn accepts_resource(&self, resource: &str) -> bool {
        resource == self.resource(true) || resource == self.resource(false)
    }

    fn cookie_name(&self) -> &'static str {
        if self.issuer.secure() {
            "__Host-crumbs-login"
        } else {
            "crumbs-login"
        }
    }

    fn cookie(&self, value: &str, max_age: u16) -> String {
        let secure = if self.issuer.secure() { "; Secure" } else { "" };
        format!(
            "{}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}",
            self.cookie_name()
        )
    }
}

/// Failure to prepare the password-verification service.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("failed to prepare password verification")]
    Password(#[from] UserError),
    #[error("password verification task failed")]
    Task(#[from] tokio::task::JoinError),
}

pub(crate) async fn require_access<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    mut request: Request<Body>,
    next: Next,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let mcp = request.uri().path() == "/mcp" || request.uri().path().starts_with("/mcp/");
    let required_scope = if mcp {
        None
    } else if request.method() == axum::http::Method::GET
        || request.method() == axum::http::Method::HEAD
    {
        Some(FoodScopes::Read)
    } else {
        Some(FoodScopes::Write)
    };
    let Some(token) = protocol::bearer(request.headers()) else {
        return challenge(&server, mcp, required_scope);
    };
    let Ok(now) = protocol::now() else {
        return internal_error();
    };
    match server
        .store
        .authenticate_access(&protocol::digest(token), &server.resource(mcp), now)
        .await
    {
        Ok(Some(session)) => {
            let scopes = session.scope;
            if let Some(required) = required_scope
                && !scopes.includes(required)
            {
                return insufficient_scope(&server, mcp, required);
            }
            let mcp_authorization = mcp.then(|| McpAuthorization::new(scopes));
            if let Some(authorization) = &mcp_authorization {
                request.extensions_mut().insert(authorization.clone());
            }
            request.extensions_mut().insert(session);
            let response = next.run(request).await;
            // Denied calls finish before emitting notifications. With JSON responses enabled,
            // rmcp waits for this terminal result before returning the HTTP response.
            match mcp_authorization.and_then(|authorization| authorization.challenge()) {
                Some(required) => scope_challenge(&server, mcp, required, response),
                None => response,
            }
        }
        Ok(None) => challenge(&server, mcp, required_scope),
        Err(_) => internal_error(),
    }
}

pub(crate) async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers
        .entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(
            "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("no-referrer"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

async fn metadata<S>(State(server): State<Arc<AuthorizationServer<S>>>) -> Json<serde_json::Value>
where
    S: Users + OAuthStore + Send + 'static,
{
    Json(json!({
        "issuer": server.issuer.as_str(),
        "authorization_endpoint": server.issuer.endpoint("/oauth/authorize"),
        "token_endpoint": server.issuer.endpoint("/oauth/token"),
        "registration_endpoint": server.issuer.endpoint("/oauth/register"),
        "revocation_endpoint": server.issuer.endpoint("/oauth/revoke"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_basic", "client_secret_post"],
        "revocation_endpoint_auth_methods_supported": ["none", "client_secret_basic", "client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": SUPPORTED_SCOPES,
        "authorization_response_iss_parameter_supported": true,
    }))
}

async fn mcp_metadata<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
) -> Json<serde_json::Value>
where
    S: Users + OAuthStore + Send + 'static,
{
    protected_metadata(&server, true)
}

async fn rest_metadata<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
) -> Json<serde_json::Value>
where
    S: Users + OAuthStore + Send + 'static,
{
    protected_metadata(&server, false)
}

fn protected_metadata<S>(server: &AuthorizationServer<S>, mcp: bool) -> Json<serde_json::Value>
where
    S: Users + OAuthStore + Send + 'static,
{
    Json(json!({
        "resource": server.resource(mcp),
        "authorization_servers": [server.issuer.as_str()],
        "scopes_supported": SUPPORTED_SCOPES,
        "bearer_methods_supported": ["header"],
        "resource_name": "Crumbs food library",
    }))
}

async fn throttle<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    request: Request<Body>,
    next: Next,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let path = request.uri().path();
    let (bucket, maximum) = match path {
        "/oauth/register" => ("registration", 10),
        "/oauth/authorize" => ("authorization", 60),
        "/oauth/login" => ("login-ip", 30),
        "/oauth/token" | "/oauth/revoke" => ("token", 120),
        _ => return next.run(request).await,
    };
    // Only the socket peer is trusted. Forwarded headers are attacker-controlled.
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0.ip().to_string())
        .unwrap_or_else(|| "local".to_owned());
    if !server
        .limits
        .allow("global", "oauth", 600, Duration::from_secs(60))
        || !server
            .limits
            .allow(bucket, &peer, maximum, Duration::from_secs(60))
    {
        return rate_limited();
    }
    next.run(request).await
}

fn challenge<S>(
    server: &AuthorizationServer<S>,
    mcp: bool,
    required_scope: Option<FoodScopes>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let metadata = server.issuer.endpoint(if mcp {
        "/.well-known/oauth-protected-resource/mcp"
    } else {
        "/.well-known/oauth-protected-resource"
    });
    let value = match required_scope {
        Some(scope) => format!(
            "Bearer resource_metadata=\"{metadata}\", scope=\"{}\"",
            scope.as_str()
        ),
        None => format!("Bearer resource_metadata=\"{metadata}\""),
    };
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, value)],
    )
        .into_response()
}

fn insufficient_scope<S>(
    server: &AuthorizationServer<S>,
    mcp: bool,
    required: FoodScopes,
) -> Response {
    scope_challenge(
        server,
        mcp,
        required,
        oauth_error(StatusCode::FORBIDDEN, "insufficient_scope"),
    )
}

fn scope_challenge<S>(
    server: &AuthorizationServer<S>,
    mcp: bool,
    required: FoodScopes,
    mut response: Response,
) -> Response {
    let metadata = server.issuer.endpoint(if mcp {
        "/.well-known/oauth-protected-resource/mcp"
    } else {
        "/.well-known/oauth-protected-resource"
    });
    let challenge = format!(
        "Bearer error=\"insufficient_scope\", scope=\"{}\", resource_metadata=\"{metadata}\"",
        required.as_str()
    );
    let Ok(challenge) = HeaderValue::from_str(&challenge) else {
        return internal_error();
    };
    *response.status_mut() = StatusCode::FORBIDDEN;
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, challenge);
    response
}

fn oauth_error(status: StatusCode, error: &'static str) -> Response {
    (status, Json(json!({"error": error}))).into_response()
}

fn internal_error() -> Response {
    oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
}

fn rate_limited() -> Response {
    let mut response = oauth_error(StatusCode::TOO_MANY_REQUESTS, "temporarily_unavailable");
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
    response
}

fn same_origin<S>(server: &AuthorizationServer<S>, headers: &HeaderMap) -> bool {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    matches!((origins.next(), origins.next()), (Some(value), None)
        if value.to_str().ok() == Some(server.issuer.as_str()))
}

const READ_SCOPE: &str = FoodScopes::Read.as_str();
const WRITE_SCOPE: &str = FoodScopes::Write.as_str();
const SUPPORTED_SCOPES: [&str; 2] = [READ_SCOPE, WRITE_SCOPE];
const LOGIN_TTL: Duration = Duration::from_secs(10 * 60);
const CODE_TTL: Duration = Duration::from_secs(60);
const ACCESS_TTL: Duration = Duration::from_secs(10 * 60);
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[cfg(test)]
#[path = "../../unit-tests/oauth/mod.rs"]
pub(crate) mod tests;
