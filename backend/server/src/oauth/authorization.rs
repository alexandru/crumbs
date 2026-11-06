//! Browser authorization with request-bound CSRF and explicit consent.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use crumbs_database::oauth::{Approval, AuthorizationGrant, OAuthStore, PendingAuthorization};
use crumbs_database::users::{Username, Users};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::views::{self, LoginPage};

use super::{
    AuthorizationServer, CODE_TTL, FoodScopes, LOGIN_TTL, internal_error, oauth_error, protocol,
    rate_limited, same_origin,
};

#[derive(Deserialize)]
pub(super) struct AuthorizationRequest {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    resource: String,
    code_challenge: String,
    code_challenge_method: String,
    scope: Option<String>,
    state: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct LoginRequest {
    request_id: String,
    csrf_token: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    decision: Decision,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Decision {
    Approve,
    Deny,
}

pub(super) async fn authorize<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    Query(request): Query<AuthorizationRequest>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    if request.client_id.len() > 256
        || request
            .state
            .as_ref()
            .is_some_and(|state| state.len() > 1024)
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let client = match server.store.oauth_client(&request.client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => return oauth_error(StatusCode::BAD_REQUEST, "invalid_client"),
        Err(_) => return internal_error(),
    };
    let requested_redirect = match protocol::redirect_uri(&request.redirect_uri) {
        Ok(redirect) => redirect,
        Err(_) => return oauth_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    if !client.redirect_uris.iter().any(|registered| {
        protocol::redirect_uri(registered).is_ok_and(|registered| {
            protocol::redirect_matches_parsed(&registered, &requested_redirect)
        })
    }) {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let failure = if request.response_type != "code" {
        Some("unsupported_response_type")
    } else if request.code_challenge_method != "S256"
        || !protocol::valid_challenge(&request.code_challenge)
    {
        Some("invalid_request")
    } else if !server.accepts_resource(&request.resource) {
        Some("invalid_target")
    } else {
        None
    };
    if let Some(error) = failure {
        return redirect(
            &server,
            &request.redirect_uri,
            request.state.as_deref(),
            Err(error),
        );
    }
    let scopes = match request.scope.as_deref() {
        Some(scope) => match FoodScopes::parse(scope) {
            Some(scopes) => scopes,
            None => {
                return redirect(
                    &server,
                    &request.redirect_uri,
                    request.state.as_deref(),
                    Err("invalid_scope"),
                );
            }
        },
        None => FoodScopes::Read,
    };
    let (Ok(now), Ok(request_id), Ok(csrf_token)) =
        (protocol::now(), protocol::secret(), protocol::secret())
    else {
        return internal_error();
    };
    let Ok(expires_at) = protocol::expires_after(now, LOGIN_TTL) else {
        return internal_error();
    };
    let pending = PendingAuthorization {
        request_hash: protocol::digest(&request_id),
        csrf_hash: protocol::digest(&csrf_token),
        grant: AuthorizationGrant {
            client_id: client.id,
            redirect_uri: request.redirect_uri,
            resource: request.resource,
            scope: scopes,
            state: request.state,
            code_challenge: request.code_challenge,
        },
        expires_at,
    };
    if server
        .store
        .begin_authorization(&pending, now)
        .await
        .is_err()
    {
        return oauth_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
    }
    let page = render(&pending, &request_id, &csrf_token, &client.name, None);
    (
        [(header::SET_COOKIE, server.cookie(&csrf_token, 600))],
        page,
    )
        .into_response()
}

pub(super) async fn login<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    headers: HeaderMap,
    Form(form): Form<LoginRequest>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let password = Zeroizing::new(form.password);
    if !same_origin(&server, &headers) || form.request_id.len() != 43 || form.csrf_token.len() != 43
    {
        return oauth_error(StatusCode::FORBIDDEN, "invalid_request");
    }
    let Some(cookie) = login_cookie(&headers, server.cookie_name()) else {
        return oauth_error(StatusCode::FORBIDDEN, "invalid_request");
    };
    if !bool::from(cookie.as_bytes().ct_eq(form.csrf_token.as_bytes())) {
        return oauth_error(StatusCode::FORBIDDEN, "invalid_request");
    }
    let Ok(now) = protocol::now() else {
        return internal_error();
    };
    let request_hash = protocol::digest(&form.request_id);
    let csrf_hash = protocol::digest(&form.csrf_token);
    let pending = match server.store.pending_authorization(&request_hash, now).await {
        Ok(Some(pending))
            if bool::from(pending.csrf_hash.as_bytes().ct_eq(csrf_hash.as_bytes())) =>
        {
            pending
        }
        Ok(_) => return oauth_error(StatusCode::BAD_REQUEST, "invalid_request"),
        Err(_) => return internal_error(),
    };
    if matches!(form.decision, Decision::Deny) {
        return match server
            .store
            .deny_authorization(&request_hash, &csrf_hash, now)
            .await
        {
            Ok(Some(grant)) => finish(&server, &grant, Err("access_denied")),
            Ok(None) => oauth_error(StatusCode::BAD_REQUEST, "invalid_request"),
            Err(_) => internal_error(),
        };
    }
    let username = Username::new(form.username);
    let account_key = username
        .as_ref()
        .map(Username::as_str)
        .unwrap_or("invalid-username");
    if !server
        .limits
        .allow("login-account", account_key, 10, Duration::from_secs(300))
    {
        return rate_limited();
    }
    let Ok(permit) = Arc::clone(&server.password_slots).try_acquire_owned() else {
        return rate_limited();
    };
    let credentials = match username {
        Ok(username) => match server.store.user_by_username(&username).await {
            Ok(credentials) => credentials,
            Err(_) => return internal_error(),
        },
        Err(_) => None,
    };
    let hash = credentials
        .as_ref()
        .map(|credentials| credentials.password_hash.clone())
        .unwrap_or_else(|| server.dummy_password.clone());
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash.verify(&password)
    })
    .await;
    let credentials = match (verified, credentials) {
        (Ok(Ok(true)), Some(credentials)) if credentials.user.active => credentials,
        _ => {
            let client = match server.store.oauth_client(&pending.grant.client_id).await {
                Ok(Some(client)) => client,
                _ => return internal_error(),
            };
            return (
                StatusCode::UNAUTHORIZED,
                render(
                    &pending,
                    &form.request_id,
                    &form.csrf_token,
                    &client.name,
                    Some("The username or password is incorrect."),
                ),
            )
                .into_response();
        }
    };
    let (Ok(code), Ok(now)) = (protocol::secret(), protocol::now()) else {
        return internal_error();
    };
    let Ok(code_expires_at) = protocol::expires_after(now, CODE_TTL) else {
        return internal_error();
    };
    let approval = Approval {
        request_hash,
        csrf_hash,
        user_id: credentials.user.id,
        password_hash: credentials.password_hash,
        code_hash: protocol::digest(&code),
        code_expires_at,
    };
    match server.store.approve_authorization(&approval, now).await {
        Ok(Some(grant)) => finish(&server, &grant, Ok(&code)),
        Ok(None) => oauth_error(StatusCode::BAD_REQUEST, "invalid_request"),
        Err(_) => internal_error(),
    }
}

fn render(
    pending: &PendingAuthorization,
    request_id: &str,
    csrf_token: &str,
    client_name: &str,
    error: Option<&str>,
) -> Response {
    let origin = match url::Url::parse(&pending.grant.redirect_uri) {
        Ok(url) => url.origin().ascii_serialization(),
        Err(_) => return internal_error(),
    };
    let content_security_policy = format!(
        "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self' {origin}; frame-ancestors 'none'; base-uri 'none'"
    );
    let Ok(content_security_policy) = HeaderValue::from_str(&content_security_policy) else {
        return internal_error();
    };
    let mut response = views::login(LoginPage {
        request_id,
        csrf_token,
        client_name,
        redirect_origin: &origin,
        scopes: pending.grant.scope,
        error,
    })
    .into_response();
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, content_security_policy);
    response
}

fn login_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|value| value.trim().split_once('='))
        .filter_map(|(key, value)| (key == name).then_some(value));
    match (values.next(), values.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

fn finish<S>(
    server: &AuthorizationServer<S>,
    grant: &AuthorizationGrant,
    result: Result<&str, &str>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    (
        [(header::SET_COOKIE, server.cookie("", 0))],
        redirect(server, &grant.redirect_uri, grant.state.as_deref(), result),
    )
        .into_response()
}

fn redirect<S>(
    server: &AuthorizationServer<S>,
    uri: &str,
    state: Option<&str>,
    result: Result<&str, &str>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    match protocol::redirect_response(uri, state, server.issuer.as_str(), result) {
        Ok(location) => Redirect::to(&location).into_response(),
        Err(_) => internal_error(),
    }
}
