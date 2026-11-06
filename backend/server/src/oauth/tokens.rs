//! Client registration, token issuance, and grant revocation.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crumbs_database::oauth::{
    AuthStoreError, ClientAuthentication, CodeRedemption, OAuthClient, OAuthStore, TokenPair,
};
use crumbs_database::users::Users;
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::{
    ACCESS_TTL, AuthorizationServer, FoodScopes, SESSION_TTL, internal_error, oauth_error, protocol,
};

#[derive(Deserialize)]
pub(super) struct RegistrationRequest {
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
    scope: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TokenRequest {
    grant_type: String,
    client_id: Option<String>,
    client_secret: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    resource: Option<String>,
    scope: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RevocationRequest {
    token: String,
    client_id: Option<String>,
    client_secret: Option<String>,
}

pub(super) async fn register<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    Json(request): Json<RegistrationRequest>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let name = request
        .client_name
        .unwrap_or_else(|| "MCP client".to_owned());
    let method_name = request
        .token_endpoint_auth_method
        .as_deref()
        .unwrap_or("client_secret_basic");
    let Some(method) = ClientAuthenticationMethod::parse(method_name) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata");
    };
    let grants = request
        .grant_types
        .unwrap_or_else(|| vec!["authorization_code".to_owned()]);
    let scopes = match request.scope.as_deref() {
        Some(scope) => FoodScopes::parse(scope),
        None => Some(FoodScopes::Read),
    };
    let Some(scopes) = scopes else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata");
    };
    if name.trim().is_empty()
        || name.len() > 100
        || name.chars().any(char::is_control)
        || request.redirect_uris.is_empty()
        || request.redirect_uris.len() > 8
        || request
            .redirect_uris
            .iter()
            .any(|uri| protocol::redirect_uri(uri).is_err())
        || !grants.iter().any(|grant| grant == "authorization_code")
        || grants
            .iter()
            .any(|grant| !matches!(grant.as_str(), "authorization_code" | "refresh_token"))
        || request
            .response_types
            .as_ref()
            .is_some_and(|types| types.as_slice() != ["code"])
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata");
    }
    let (Ok(id), Ok(secret), Ok(now)) = (protocol::secret(), protocol::secret(), protocol::now())
    else {
        return internal_error();
    };
    let secret = Zeroizing::new(secret);
    let authentication = match method {
        ClientAuthenticationMethod::Public => ClientAuthentication::Public,
        ClientAuthenticationMethod::SecretBasic => {
            ClientAuthentication::SecretBasic(protocol::digest(&secret))
        }
        ClientAuthenticationMethod::SecretPost => {
            ClientAuthentication::SecretPost(protocol::digest(&secret))
        }
    };
    let client = OAuthClient {
        id,
        name,
        redirect_uris: request.redirect_uris,
        authentication,
        allow_refresh: grants.iter().any(|grant| grant == "refresh_token"),
    };
    if server.store.register_client(&client, now).await.is_err() {
        return oauth_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
    }
    let mut response = json!({
        "client_id": client.id,
        "client_id_issued_at": now / 1000,
        "client_name": client.name,
        "redirect_uris": client.redirect_uris,
        "token_endpoint_auth_method": method.as_str(),
        "grant_types": grants,
        "response_types": ["code"],
        "scope": scopes.as_str(),
    });
    if !matches!(method, ClientAuthenticationMethod::Public) {
        response["client_secret"] = json!(secret.as_str());
        response["client_secret_expires_at"] = json!(0);
    }
    (StatusCode::CREATED, Json(response)).into_response()
}

pub(super) async fn token<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    headers: HeaderMap,
    Form(request): Form<TokenRequest>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let client = match authenticate_client(
        &server,
        &headers,
        request.client_id.as_deref(),
        request.client_secret.as_deref(),
    )
    .await
    {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let Some(resource) = request
        .resource
        .as_deref()
        .filter(|value| server.accepts_resource(value))
    else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_target");
    };
    let requested_scopes = match request.scope.as_deref() {
        Some(scope) => match FoodScopes::parse(scope) {
            Some(scopes) => Some(scopes),
            None => return oauth_error(StatusCode::BAD_REQUEST, "invalid_scope"),
        },
        None => None,
    };
    let (Ok(access), Ok(refresh), Ok(now)) =
        (protocol::secret(), protocol::secret(), protocol::now())
    else {
        return internal_error();
    };
    let access = Zeroizing::new(access);
    let refresh = Zeroizing::new(refresh);
    let (Ok(access_expires_at), Ok(session_expires_at)) = (
        protocol::expires_after(now, ACCESS_TTL),
        protocol::expires_after(now, SESSION_TTL),
    ) else {
        return internal_error();
    };
    let pair = TokenPair {
        access_hash: protocol::digest(&access),
        refresh_hash: protocol::digest(&refresh),
        access_expires_at,
        session_expires_at,
    };
    let session = match request.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(redirect_uri), Some(verifier)) =
                (request.code, request.redirect_uri, request.code_verifier)
            else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
            };
            let Ok(code_challenge) = protocol::pkce_challenge(&verifier) else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            };
            if code.len() != 43 {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            server
                .store
                .redeem_code(
                    &CodeRedemption {
                        code_hash: protocol::digest(&code),
                        client_id: client.id,
                        redirect_uri,
                        resource: resource.to_owned(),
                        code_challenge,
                    },
                    requested_scopes.map(FoodScopes::as_str),
                    &pair,
                    now,
                )
                .await
        }
        "refresh_token" if client.allow_refresh => {
            let Some(token) = request.refresh_token.filter(|token| token.len() == 43) else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            };
            server
                .store
                .rotate_refresh(
                    &protocol::digest(&token),
                    &client.id,
                    resource,
                    requested_scopes.map(FoodScopes::as_str),
                    &pair,
                    now,
                )
                .await
        }
        "refresh_token" => return oauth_error(StatusCode::BAD_REQUEST, "unauthorized_client"),
        _ => return oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    };
    match session {
        Ok(Some(session)) => {
            let Some(scopes) = FoodScopes::parse(&session.scope) else {
                return internal_error();
            };
            let mut response = json!({
                "access_token": access.as_str(), "token_type": "Bearer",
                "expires_in": (access_expires_at.min(session.expires_at) - now) / 1000,
                "scope": scopes.as_str(),
            });
            if client.allow_refresh {
                response["refresh_token"] = json!(refresh.as_str());
            }
            Json(response).into_response()
        }
        Ok(None) => oauth_error(StatusCode::BAD_REQUEST, "invalid_grant"),
        Err(AuthStoreError::InvalidScope) => oauth_error(StatusCode::BAD_REQUEST, "invalid_scope"),
        Err(_) => internal_error(),
    }
}

pub(super) async fn revoke<S>(
    State(server): State<Arc<AuthorizationServer<S>>>,
    headers: HeaderMap,
    Form(request): Form<RevocationRequest>,
) -> Response
where
    S: Users + OAuthStore + Send + 'static,
{
    let client = match authenticate_client(
        &server,
        &headers,
        request.client_id.as_deref(),
        request.client_secret.as_deref(),
    )
    .await
    {
        Ok(client) => client,
        Err(response) => return *response,
    };
    if request.token.len() > 256 {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    match server
        .store
        .revoke_token(&protocol::digest(&request.token), &client.id)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => internal_error(),
    }
}

#[derive(Clone, Copy)]
enum ClientAuthenticationMethod {
    Public,
    SecretBasic,
    SecretPost,
}

impl ClientAuthenticationMethod {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::Public),
            "client_secret_basic" => Some(Self::SecretBasic),
            "client_secret_post" => Some(Self::SecretPost),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Public => "none",
            Self::SecretBasic => "client_secret_basic",
            Self::SecretPost => "client_secret_post",
        }
    }
}

async fn authenticate_client<S>(
    server: &AuthorizationServer<S>,
    headers: &HeaderMap,
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> Result<OAuthClient, Box<Response>>
where
    S: Users + OAuthStore + Send + 'static,
{
    let basic = basic_credentials(headers)?;
    let id = match (&basic, client_id) {
        (Some((id, _)), Some(body_id)) if id != body_id => {
            return Err(Box::new(invalid_client()));
        }
        (Some((id, _)), _) => id.as_str(),
        (None, Some(id)) => id,
        (None, None) => return Err(Box::new(invalid_client())),
    };
    if id.len() > 256 || client_secret.is_some_and(|secret| secret.len() > 256) {
        return Err(Box::new(invalid_client()));
    }
    let client = server
        .store
        .oauth_client(id)
        .await
        .map_err(|_| Box::new(internal_error()))?
        .ok_or_else(|| Box::new(invalid_client()))?;
    let valid = match &client.authentication {
        ClientAuthentication::Public => basic.is_none() && client_secret.is_none(),
        ClientAuthentication::SecretBasic(hash) => {
            client_secret.is_none()
                && basic.as_ref().is_some_and(|(_, secret)| {
                    bool::from(hash.as_bytes().ct_eq(protocol::digest(secret).as_bytes()))
                })
        }
        ClientAuthentication::SecretPost(hash) => {
            basic.is_none()
                && client_secret.is_some_and(|secret| {
                    bool::from(hash.as_bytes().ct_eq(protocol::digest(secret).as_bytes()))
                })
        }
    };
    if valid {
        Ok(client)
    } else {
        Err(Box::new(invalid_client()))
    }
}

fn basic_credentials(
    headers: &HeaderMap,
) -> Result<Option<(String, Zeroizing<String>)>, Box<Response>> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = match (values.next(), values.next()) {
        (None, None) => return Ok(None),
        (Some(value), None) => value.to_str().map_err(|_| Box::new(invalid_client()))?,
        _ => return Err(Box::new(invalid_client())),
    };
    if value.len() > 1024 {
        return Err(Box::new(invalid_client()));
    }
    let (scheme, credentials) = value
        .split_once(' ')
        .ok_or_else(|| Box::new(invalid_client()))?;
    if !scheme.eq_ignore_ascii_case("Basic") {
        return Err(Box::new(invalid_client()));
    }
    let decoded = Zeroizing::new(
        STANDARD
            .decode(credentials.trim_start_matches(' '))
            .map_err(|_| Box::new(invalid_client()))?,
    );
    let text = std::str::from_utf8(&decoded).map_err(|_| Box::new(invalid_client()))?;
    let (id, secret) = text
        .split_once(':')
        .ok_or_else(|| Box::new(invalid_client()))?;
    Ok(Some((
        decode_component(id)?,
        Zeroizing::new(decode_component(secret)?),
    )))
}

fn decode_component(value: &str) -> Result<String, Box<Response>> {
    let encoded = Zeroizing::new(format!("v={value}"));
    let mut pairs = url::form_urlencoded::parse(encoded.as_bytes());
    match (pairs.next(), pairs.next()) {
        (Some((key, value)), None) if key == "v" => Ok(value.into_owned()),
        _ => Err(Box::new(invalid_client())),
    }
}

fn invalid_client() -> Response {
    (
        [(header::WWW_AUTHENTICATE, "Basic realm=\"crumbs\"")],
        oauth_error(StatusCode::UNAUTHORIZED, "invalid_client"),
    )
        .into_response()
}
