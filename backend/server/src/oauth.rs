//! OAuth discovery, authorization, and access-token enforcement.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use crumbs_database::oauth::OAuthStore;
use crumbs_database::users::{PasswordHash, UserError, Users};
use serde_json::json;
use tokio::sync::Semaphore;

use crate::{IssuerUrl, OAuthConfig, oauth_views};

mod authorization;
mod limits;
pub(crate) mod protocol;
mod tokens;

pub(crate) struct AuthorizationServer<S> {
    store: Arc<S>,
    issuer: IssuerUrl,
    dummy_password: PasswordHash,
    password_slots: Arc<Semaphore>,
    limits: limits::Limits,
}

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

pub(crate) fn routes<S>(server: Arc<AuthorizationServer<S>>) -> Router
where
    S: Users + OAuthStore + Send + 'static,
{
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(metadata::<S>),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            axum::routing::get(mcp_metadata::<S>),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            axum::routing::get(rest_metadata::<S>),
        )
        .route(
            "/oauth/register",
            axum::routing::post(tokens::register::<S>),
        )
        .route(
            "/oauth/authorize",
            axum::routing::get(authorization::authorize::<S>),
        )
        .route(
            "/oauth/login",
            axum::routing::post(authorization::login::<S>),
        )
        .route("/oauth/token", axum::routing::post(tokens::token::<S>))
        .route("/oauth/revoke", axum::routing::post(tokens::revoke::<S>))
        .route(
            "/assets/login.css",
            axum::routing::get(async || oauth_views::stylesheet()),
        )
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&server),
            throttle::<S>,
        ))
        .with_state(server)
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
    let Some(token) = protocol::bearer(request.headers()) else {
        return challenge(&server, mcp);
    };
    let Ok(now) = protocol::now() else {
        return internal_error();
    };
    match server
        .store
        .authenticate_access(&protocol::digest(token), &server.resource(mcp), now)
        .await
    {
        Ok(Some(session)) if session.scope == READ_SCOPE => {
            request.extensions_mut().insert(session);
            next.run(request).await
        }
        Ok(Some(_)) => oauth_error(StatusCode::FORBIDDEN, "insufficient_scope"),
        Ok(None) => challenge(&server, mcp),
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
        "scopes_supported": [READ_SCOPE],
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
        "scopes_supported": [READ_SCOPE],
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

fn challenge<S>(server: &AuthorizationServer<S>, mcp: bool) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let metadata = server.issuer.endpoint(if mcp {
        "/.well-known/oauth-protected-resource/mcp"
    } else {
        "/.well-known/oauth-protected-resource"
    });
    let value = format!("Bearer resource_metadata=\"{metadata}\", scope=\"{READ_SCOPE}\"");
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, value)],
    )
        .into_response()
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

const READ_SCOPE: &str = "crumbs:read";
const LOGIN_TTL: Duration = Duration::from_secs(10 * 60);
const CODE_TTL: Duration = Duration::from_secs(60);
const ACCESS_TTL: Duration = Duration::from_secs(10 * 60);
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[cfg(test)]
#[path = "../unit-tests/oauth.rs"]
pub(crate) mod tests;
