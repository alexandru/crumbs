use std::error::Error;
use std::sync::{Arc, OnceLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use crumbs_database::identity::{
    Approval, AuthorizationGrant, ClientAuthentication, CodeRedemption, OAuthClient, OAuthStore,
    PendingAuthorization, TokenPair,
};
use crumbs_database::identity::{PasswordHash, UserError, UserId, Username, Users};
use crumbs_database::{Database, DatabaseConfig};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use url::form_urlencoded;

use super::{AuthorizationServer, FoodScopes, protocol};
use crate::{IssuerUrl, OAuthConfig};

pub(crate) const TEST_ISSUER: &str = "https://crumbs.example.com";
pub(crate) const TEST_MCP_RESOURCE: &str = "https://crumbs.example.com/mcp";
pub(crate) const TEST_REST_RESOURCE: &str = "https://crumbs.example.com/";
pub(crate) const TEST_USERNAME: &str = "alice";
pub(crate) const TEST_PASSWORD: &str = "correct horse battery staple";
pub(crate) const TEST_MCP_ACCESS_TOKEN: &str = "test-mcp-access-token";
pub(crate) const TEST_REST_ACCESS_TOKEN: &str = "test-rest-access-token";
pub(crate) const TEST_MCP_WRITE_ACCESS_TOKEN: &str = "test-mcp-write-access-token";
pub(crate) const TEST_REST_WRITE_ACCESS_TOKEN: &str = "test-rest-write-access-token";
const TEST_MCP_REFRESH_TOKEN: &str = "test-mcp-refresh-token";
const TEST_REST_REFRESH_TOKEN: &str = "test-rest-refresh-token";
const TEST_MCP_WRITE_REFRESH_TOKEN: &str = "test-mcp-write-refresh-token";
const TEST_REST_WRITE_REFRESH_TOKEN: &str = "test-rest-write-refresh-token";
const TEST_REDIRECT_URI: &str = "https://client.example/callback";
const TEST_VERIFIER: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~";

type TestResult<T> = Result<T, Box<dyn Error>>;

#[test]
fn food_scopes_parse_supported_values_and_normalize_order_and_duplicates() {
    for (value, expected, can_read, can_write) in [
        ("crumbs:read", "crumbs:read", true, false),
        ("crumbs:write", "crumbs:write", false, true),
        (
            "crumbs:write crumbs:read crumbs:write",
            "crumbs:read crumbs:write",
            true,
            true,
        ),
    ] {
        let scopes = FoodScopes::parse(value);
        assert_eq!(scopes.map(FoodScopes::as_str), Some(expected));
        assert_eq!(scopes.map(FoodScopes::can_read), Some(can_read));
        assert_eq!(scopes.map(FoodScopes::can_write), Some(can_write));
    }
}

#[test]
fn food_scopes_reject_empty_and_unknown_values() {
    for value in ["", "  \t", "crumbs:admin", "crumbs:read,crumbs:write"] {
        assert!(FoodScopes::parse(value).is_none(), "accepted {value:?}");
    }
}

#[tokio::test]
async fn rest_write_requires_write_scope_and_reports_the_required_scope() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let database = Arc::new(
        Database::open(DatabaseConfig {
            path: directory.path().join("oauth.db"),
        })
        .await?,
    );
    let authorization = test_authorization_server(database).await?;
    let router = Router::new()
        .route(
            "/custom-foods",
            axum::routing::post(|| async { StatusCode::NO_CONTENT }),
        )
        .layer(axum::middleware::from_fn_with_state(
            authorization,
            super::require_access::<Database>,
        ));

    let missing = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/custom-foods")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert!(
        missing
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .is_some_and(|value| value
                .to_str()
                .is_ok_and(|value| value.contains("scope=\"crumbs:write\"")))
    );

    let read_only = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/custom-foods")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {TEST_REST_ACCESS_TOKEN}"),
                )
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(read_only.status(), StatusCode::FORBIDDEN);
    assert!(
        read_only
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .is_some_and(|value| value
                .to_str()
                .is_ok_and(|value| value.contains("scope=\"crumbs:write\"")))
    );

    let read_write = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/custom-foods")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {TEST_REST_WRITE_ACCESS_TOKEN}"),
                )
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(read_write.status(), StatusCode::NO_CONTENT);
    Ok(())
}

pub(crate) async fn test_authorization_server(
    store: Arc<Database>,
) -> TestResult<Arc<AuthorizationServer<Database>>> {
    seed_session(
        &store,
        "test-mcp-client",
        TEST_MCP_RESOURCE,
        TEST_MCP_ACCESS_TOKEN,
        TEST_MCP_REFRESH_TOKEN,
    )
    .await?;
    seed_session(
        &store,
        "test-rest-client",
        TEST_REST_RESOURCE,
        TEST_REST_ACCESS_TOKEN,
        TEST_REST_REFRESH_TOKEN,
    )
    .await?;
    seed_session_with_scope(
        &store,
        "test-mcp-write-client",
        TEST_MCP_RESOURCE,
        TEST_MCP_WRITE_ACCESS_TOKEN,
        TEST_MCP_WRITE_REFRESH_TOKEN,
        FoodScopes::ReadWrite,
    )
    .await?;
    seed_session_with_scope(
        &store,
        "test-rest-write-client",
        TEST_REST_RESOURCE,
        TEST_REST_WRITE_ACCESS_TOKEN,
        TEST_REST_WRITE_REFRESH_TOKEN,
        FoodScopes::ReadWrite,
    )
    .await?;
    Ok(AuthorizationServer::new(store, oauth_config()?).await?)
}

pub(crate) async fn seed_session(
    store: &Database,
    client_id: &str,
    resource: &str,
    access_token: &str,
    refresh_token: &str,
) -> TestResult<()> {
    seed_session_with_scope(
        store,
        client_id,
        resource,
        access_token,
        refresh_token,
        FoodScopes::Read,
    )
    .await
}

pub(crate) async fn seed_session_with_scope(
    store: &Database,
    client_id: &str,
    resource: &str,
    access_token: &str,
    refresh_token: &str,
    scope: FoodScopes,
) -> TestResult<()> {
    let credentials = test_user(store).await?;
    seed_session_for_user(
        store,
        client_id,
        resource,
        access_token,
        refresh_token,
        scope,
        credentials,
    )
    .await
}

pub(crate) async fn seed_session_for_user(
    store: &Database,
    client_id: &str,
    resource: &str,
    access_token: &str,
    refresh_token: &str,
    scope: FoodScopes,
    (user_id, password_hash): (UserId, PasswordHash),
) -> TestResult<()> {
    let now = protocol::now()?;
    let request_id = protocol::secret()?;
    let csrf_token = protocol::secret()?;
    let code = protocol::secret()?;
    let challenge = protocol::pkce_challenge(TEST_VERIFIER)?;
    let client = OAuthClient {
        id: client_id.to_owned(),
        name: "Crumbs test client".to_owned(),
        redirect_uris: vec![TEST_REDIRECT_URI.to_owned()],
        authentication: ClientAuthentication::Public,
        allow_refresh: true,
    };
    store.register_client(&client, now).await?;

    let request_hash = protocol::digest(&request_id);
    let csrf_hash = protocol::digest(&csrf_token);
    store
        .begin_authorization(
            &PendingAuthorization {
                request_hash,
                csrf_hash,
                grant: AuthorizationGrant {
                    client_id: client.id.clone(),
                    redirect_uri: TEST_REDIRECT_URI.to_owned(),
                    resource: resource.to_owned(),
                    scope,
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
                user_id,
                password_hash,
                code_hash: protocol::digest(&code),
                code_expires_at: now + 60_000,
            },
            now,
        )
        .await?;
    store
        .redeem_code(
            &CodeRedemption {
                code_hash: protocol::digest(&code),
                client_id: client.id,
                redirect_uri: TEST_REDIRECT_URI.to_owned(),
                resource: resource.to_owned(),
                code_challenge: challenge,
            },
            None,
            &TokenPair {
                access_hash: protocol::digest(access_token),
                refresh_hash: protocol::digest(refresh_token),
                access_expires_at: now + 600_000,
                session_expires_at: now + 2_592_000_000,
            },
            now,
        )
        .await?;
    Ok(())
}

pub(crate) fn oauth_config() -> TestResult<OAuthConfig> {
    Ok(OAuthConfig {
        issuer: IssuerUrl::new(TEST_ISSUER)?,
    })
}

pub(crate) struct Application {
    _directory: TempDir,
    database: Arc<Database>,
    router: Router,
}

impl Application {
    async fn new() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let database = Arc::new(
            Database::open(DatabaseConfig {
                path: directory.path().join("crumbs.db"),
            })
            .await?,
        );
        let authorization = test_authorization_server(Arc::clone(&database)).await?;
        let router =
            crate::routes::application(Arc::clone(&database), crate::mcp::config(), authorization);
        Ok(Self {
            _directory: directory,
            database,
            router,
        })
    }

    async fn request(&self, request: Request<Body>) -> TestResult<ResponseBody> {
        request_router(&self.router, request).await
    }
}

async fn test_user(store: &Database) -> TestResult<(UserId, PasswordHash)> {
    let username = Username::new(TEST_USERNAME)?;
    match store.user_by_username(&username).await? {
        Some(credentials) => Ok((credentials.user.id, credentials.password_hash)),
        None => {
            let password_hash = test_password_hash()?;
            let user = store.create_user(&username, &password_hash).await?;
            Ok((user.id, password_hash))
        }
    }
}

fn test_password_hash() -> TestResult<PasswordHash> {
    static HASH: OnceLock<Result<PasswordHash, UserError>> = OnceLock::new();
    match HASH.get_or_init(|| PasswordHash::hash(TEST_PASSWORD)) {
        Ok(hash) => Ok(hash.clone()),
        Err(error) => Err(std::io::Error::other(error.to_string()).into()),
    }
}

struct ResponseBody {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn request_router(router: &Router, request: Request<Body>) -> TestResult<ResponseBody> {
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(ResponseBody {
        status,
        headers,
        body: String::from_utf8(body.to_vec())?,
    })
}

fn get_request(uri: &str) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())?)
}

fn bearer_request(uri: &str, token: &str) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?)
}

fn mcp_request(uri: &str, token: &str, body: Value) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::HOST, "foods.example")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(serde_json::to_vec(&body)?))?)
}

fn json_request(method: &str, path: &str, body: Value) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?)
}

fn form_request(
    path: &str,
    fields: &[(&str, &str)],
    origin: Option<&str>,
    cookie: Option<&str>,
) -> TestResult<Request<Body>> {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(fields.iter().copied());
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    Ok(builder.body(Body::from(serializer.finish()))?)
}

fn form_query(fields: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(fields.iter().copied());
    serializer.finish()
}

fn page_value<'a>(page: &'a str, name: &str) -> Option<&'a str> {
    let name_attribute = format!("name=\"{name}\"");
    let after_name = page.split_once(&name_attribute)?.1;
    let after_value = after_name.split_once("value=\"")?.1;
    after_value.split_once('"').map(|(value, _)| value)
}

fn cookie_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::SET_COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .next()
        .map(str::to_owned)
}

#[tokio::test]
async fn discovery_registration_and_login_complete_the_pkce_flow() -> TestResult<()> {
    let app = Application::new().await?;

    let discovery = app
        .request(get_request("/.well-known/oauth-authorization-server")?)
        .await?;
    assert_eq!(discovery.status, StatusCode::OK);
    assert_eq!(
        discovery
            .headers
            .get(header::REFERRER_POLICY)
            .and_then(|value| value.to_str().ok()),
        Some("no-referrer")
    );
    assert_eq!(
        discovery
            .headers
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|value| value.to_str().ok()),
        Some(
            "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"
        )
    );
    let metadata: Value = serde_json::from_str(&discovery.body)?;
    assert_eq!(metadata["issuer"], TEST_ISSUER);
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        json!(["S256"])
    );
    assert_eq!(
        metadata["scopes_supported"],
        json!(["crumbs:read", "crumbs:write"])
    );

    let mcp_resource = app
        .request(get_request("/.well-known/oauth-protected-resource/mcp")?)
        .await?;
    let rest_resource = app
        .request(get_request("/.well-known/oauth-protected-resource")?)
        .await?;
    assert_eq!(
        serde_json::from_str::<Value>(&mcp_resource.body)?["resource"],
        TEST_MCP_RESOURCE
    );
    assert_eq!(
        serde_json::from_str::<Value>(&mcp_resource.body)?["scopes_supported"],
        json!(["crumbs:read", "crumbs:write"])
    );
    assert_eq!(
        serde_json::from_str::<Value>(&rest_resource.body)?["resource"],
        TEST_REST_RESOURCE
    );
    assert_eq!(
        serde_json::from_str::<Value>(&rest_resource.body)?["scopes_supported"],
        json!(["crumbs:read", "crumbs:write"])
    );

    let registration = app
        .request(json_request(
            "POST",
            "/oauth/register",
            json!({
                "redirect_uris": [TEST_REDIRECT_URI],
                "client_name": "Food search client",
                "token_endpoint_auth_method": "none",
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"]
            }),
        )?)
        .await?;
    assert_eq!(
        registration.status,
        StatusCode::CREATED,
        "{}",
        registration.body
    );
    let client: Value = serde_json::from_str(&registration.body)?;
    assert!(client["client_id"].is_string());
    assert!(client.get("client_secret").is_none());
    assert_eq!(client["scope"], "crumbs:read");

    let verifier = TEST_VERIFIER;
    let challenge = protocol::pkce_challenge(verifier)?;
    let authorization_query = form_query(&[
        (
            "client_id",
            client["client_id"].as_str().ok_or("client id missing")?,
        ),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("response_type", "code"),
        ("resource", TEST_MCP_RESOURCE),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("state", "client-state-123"),
    ]);
    let page = app
        .request(get_request(&format!(
            "/oauth/authorize?{authorization_query}"
        ))?)
        .await?;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("Food search client"));
    assert!(page.body.contains("Read the food library"));
    assert_eq!(
        page.headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    assert!(page.headers.contains_key(header::CONTENT_SECURITY_POLICY));
    assert_eq!(
        page.headers
            .get(header::REFERRER_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("same-origin")
    );
    assert_eq!(
        page.headers
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|value| value.to_str().ok()),
        Some(
            "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self' https://client.example; frame-ancestors 'none'; base-uri 'none'"
        )
    );
    assert_eq!(
        page.headers
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    let cookie = cookie_value(&page.headers).ok_or("login cookie missing")?;
    let set_cookie = page
        .headers
        .get(header::SET_COOKIE)
        .ok_or("Set-Cookie missing")?
        .to_str()?;
    for attribute in [
        "__Host-crumbs-login=",
        "Path=/",
        "Max-Age=600",
        "HttpOnly",
        "SameSite=Lax",
        "Secure",
    ] {
        assert!(
            set_cookie.contains(attribute),
            "missing cookie attribute {attribute:?}: {set_cookie}"
        );
    }
    let request_id = page_value(&page.body, "request_id").ok_or("request_id missing")?;
    let csrf_token = page_value(&page.body, "csrf_token").ok_or("csrf_token missing")?;

    let login = app
        .request(form_request(
            "/oauth/login",
            &[
                ("request_id", request_id),
                ("csrf_token", csrf_token),
                ("username", TEST_USERNAME),
                ("password", TEST_PASSWORD),
                ("decision", "approve"),
            ],
            Some(TEST_ISSUER),
            Some(&cookie),
        )?)
        .await?;
    assert_eq!(login.status, StatusCode::SEE_OTHER, "{}", login.body);
    let location = login
        .headers
        .get(header::LOCATION)
        .ok_or("redirect missing")?
        .to_str()?;
    let redirect = url::Url::parse(location)?;
    assert_eq!(redirect.path(), "/callback");
    let query: std::collections::HashMap<_, _> = redirect.query_pairs().into_owned().collect();
    assert_eq!(
        query.get("state").map(String::as_str),
        Some("client-state-123")
    );
    assert_eq!(query.get("iss").map(String::as_str), Some(TEST_ISSUER));
    let code = query.get("code").ok_or("authorization code missing")?;
    assert_eq!(
        login
            .headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("Max-Age=0")),
        Some(true)
    );

    let token = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                (
                    "client_id",
                    client["client_id"].as_str().ok_or("client id missing")?,
                ),
                ("code", code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", verifier),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(token.status, StatusCode::OK, "{}", token.body);
    let tokens: Value = serde_json::from_str(&token.body)?;
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["scope"], "crumbs:read");
    let access = tokens["access_token"]
        .as_str()
        .ok_or("access token missing")?;
    let refresh = tokens["refresh_token"]
        .as_str()
        .ok_or("refresh token missing")?;
    assert_eq!(access.len(), 43);
    assert_eq!(refresh.len(), 43);

    let wrong_audience = app
        .request(bearer_request("/foods/search?query=chicken", access)?)
        .await?;
    assert_eq!(wrong_audience.status, StatusCode::UNAUTHORIZED);
    assert!(
        wrong_audience
            .headers
            .get(header::WWW_AUTHENTICATE)
            .is_some_and(|v| {
                v.to_str()
                    .is_ok_and(|value| value.contains("oauth-protected-resource\""))
            })
    );

    let mcp_access = app
        .request(mcp_request(
            "/mcp",
            access,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "oauth-flow-test", "version": "1"}
                }
            }),
        )?)
        .await?;
    assert_eq!(mcp_access.status, StatusCode::OK, "{}", mcp_access.body);
    let rest_token_on_mcp = app
        .request(mcp_request(
            "/mcp",
            TEST_REST_ACCESS_TOKEN,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "wrong-audience-test", "version": "1"}
                }
            }),
        )?)
        .await?;
    assert_eq!(rest_token_on_mcp.status, StatusCode::UNAUTHORIZED);

    let rest_response = app
        .request(bearer_request(
            "/foods/search?query=chicken",
            TEST_REST_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(
        rest_response.status,
        StatusCode::OK,
        "{}",
        rest_response.body
    );

    let refresh_response = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                (
                    "client_id",
                    client["client_id"].as_str().ok_or("client id missing")?,
                ),
                ("refresh_token", refresh),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(
        refresh_response.status,
        StatusCode::OK,
        "{}",
        refresh_response.body
    );
    Ok(())
}

#[tokio::test]
async fn registration_rejects_invalid_redirect_without_redirecting_to_it() -> TestResult<()> {
    let app = Application::new().await?;
    let registration = app
        .request(json_request(
            "POST",
            "/oauth/register",
            json!({"redirect_uris":[TEST_REDIRECT_URI],"token_endpoint_auth_method":"none"}),
        )?)
        .await?;
    let client: Value = serde_json::from_str(&registration.body)?;
    let query = form_query(&[
        (
            "client_id",
            client["client_id"].as_str().ok_or("client id missing")?,
        ),
        ("redirect_uri", "https://attacker.example/callback"),
        ("response_type", "code"),
        ("resource", TEST_MCP_RESOURCE),
        ("code_challenge", "A".repeat(43).as_str()),
        ("code_challenge_method", "S256"),
        ("state", "state"),
    ]);
    let response = app
        .request(get_request(&format!("/oauth/authorize?{query}"))?)
        .await?;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert!(response.headers.get(header::LOCATION).is_none());
    Ok(())
}

#[tokio::test]
async fn authorization_and_token_requests_reject_unknown_scopes() -> TestResult<()> {
    let app = Application::new().await?;
    let registration = app
        .request(json_request(
            "POST",
            "/oauth/register",
            json!({
                "redirect_uris": [TEST_REDIRECT_URI],
                "token_endpoint_auth_method": "none"
            }),
        )?)
        .await?;
    assert_eq!(registration.status, StatusCode::CREATED);
    let client: Value = serde_json::from_str(&registration.body)?;
    let client_id = client["client_id"].as_str().ok_or("client id missing")?;

    let authorization_query = form_query(&[
        ("client_id", client_id),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("response_type", "code"),
        ("resource", TEST_REST_RESOURCE),
        ("code_challenge", "A".repeat(43).as_str()),
        ("code_challenge_method", "S256"),
        ("scope", "crumbs:admin"),
    ]);
    let authorization = app
        .request(get_request(&format!(
            "/oauth/authorize?{authorization_query}"
        ))?)
        .await?;
    assert_eq!(authorization.status, StatusCode::SEE_OTHER);
    assert!(
        authorization
            .headers
            .get(header::LOCATION)
            .is_some_and(|value| value
                .to_str()
                .is_ok_and(|value| value.contains("error=invalid_scope")))
    );

    let token = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "unsupported"),
                ("client_id", client_id),
                ("resource", TEST_REST_RESOURCE),
                ("scope", "crumbs:admin"),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(token.status, StatusCode::BAD_REQUEST);
    assert!(token.body.contains("invalid_scope"));
    Ok(())
}

#[tokio::test]
async fn login_binds_cookie_csrf_and_origin_and_hides_account_existence() -> TestResult<()> {
    let app = Application::new().await?;
    let registration = app
        .request(json_request(
            "POST",
            "/oauth/register",
            json!({"redirect_uris":[TEST_REDIRECT_URI],"token_endpoint_auth_method":"none"}),
        )?)
        .await?;
    let client: Value = serde_json::from_str(&registration.body)?;
    let challenge = protocol::pkce_challenge(TEST_VERIFIER)?;
    let query = form_query(&[
        (
            "client_id",
            client["client_id"].as_str().ok_or("client id missing")?,
        ),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("response_type", "code"),
        ("resource", TEST_MCP_RESOURCE),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
    ]);
    let page = app
        .request(get_request(&format!("/oauth/authorize?{query}"))?)
        .await?;
    let cookie = cookie_value(&page.headers).ok_or("login cookie missing")?;
    let request_id = page_value(&page.body, "request_id").ok_or("request_id missing")?;
    let csrf = page_value(&page.body, "csrf_token").ok_or("csrf token missing")?;
    let base_fields = [
        ("request_id", request_id),
        ("csrf_token", csrf),
        ("username", TEST_USERNAME),
        ("password", TEST_PASSWORD),
        ("decision", "approve"),
    ];

    let wrong_origin = app
        .request(form_request(
            "/oauth/login",
            &base_fields,
            Some("https://evil.example"),
            Some(&cookie),
        )?)
        .await?;
    assert_eq!(wrong_origin.status, StatusCode::FORBIDDEN);
    let no_cookie = app
        .request(form_request(
            "/oauth/login",
            &base_fields,
            Some(TEST_ISSUER),
            None,
        )?)
        .await?;
    assert_eq!(no_cookie.status, StatusCode::FORBIDDEN);
    let wrong_csrf = app
        .request(form_request(
            "/oauth/login",
            &[
                ("request_id", request_id),
                ("csrf_token", "z".repeat(43).as_str()),
                ("username", TEST_USERNAME),
                ("password", TEST_PASSWORD),
                ("decision", "approve"),
            ],
            Some(TEST_ISSUER),
            Some(&cookie),
        )?)
        .await?;
    assert_eq!(wrong_csrf.status, StatusCode::FORBIDDEN);

    let unknown_user = app
        .request(form_request(
            "/oauth/login",
            &[
                ("request_id", request_id),
                ("csrf_token", csrf),
                ("username", "nobody"),
                ("password", TEST_PASSWORD),
                ("decision", "approve"),
            ],
            Some(TEST_ISSUER),
            Some(&cookie),
        )?)
        .await?;
    let wrong_password = app
        .request(form_request(
            "/oauth/login",
            &[
                ("request_id", request_id),
                ("csrf_token", csrf),
                ("username", TEST_USERNAME),
                ("password", "incorrect password value"),
                ("decision", "approve"),
            ],
            Some(TEST_ISSUER),
            Some(&cookie),
        )?)
        .await?;
    assert_eq!(unknown_user.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_password.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_user.body, wrong_password.body);
    for response in [&unknown_user, &wrong_password] {
        assert_eq!(
            response
                .headers
                .get(header::REFERRER_POLICY)
                .and_then(|value| value.to_str().ok()),
            Some("same-origin")
        );
        assert_eq!(
            response
                .headers
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|value| value.to_str().ok()),
            Some(
                "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self' https://client.example; frame-ancestors 'none'; base-uri 'none'"
            )
        );
    }
    assert!(
        unknown_user
            .body
            .contains("The username or password is incorrect.")
    );
    assert!(!unknown_user.body.contains("nobody"));
    Ok(())
}

#[tokio::test]
async fn bearer_tokens_are_required_and_refresh_tokens_are_not_access_tokens() -> TestResult<()> {
    let app = Application::new().await?;

    let missing = app
        .request(get_request("/foods/search?query=chicken")?)
        .await?;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    assert!(missing.headers.get(header::WWW_AUTHENTICATE).is_some());

    let query_token = app
        .request(get_request(&format!(
            "/foods/search?query=chicken&access_token={TEST_REST_ACCESS_TOKEN}"
        ))?)
        .await?;
    assert_eq!(query_token.status, StatusCode::UNAUTHORIZED);

    let duplicate = app
        .request(
            Request::builder()
                .method("GET")
                .uri("/foods/search?query=chicken")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {TEST_REST_ACCESS_TOKEN}"),
                )
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {TEST_REST_ACCESS_TOKEN}"),
                )
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(duplicate.status, StatusCode::UNAUTHORIZED);

    let refresh = app
        .request(bearer_request(
            "/foods/search?query=chicken",
            TEST_REST_REFRESH_TOKEN,
        )?)
        .await?;
    assert_eq!(refresh.status, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn unauthenticated_mcp_request_is_rejected_before_food_search() -> TestResult<()> {
    let app = Application::new().await?;
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#))?;

    let response = app.request(request).await?;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert!(response.headers.get(header::WWW_AUTHENTICATE).is_some());
    Ok(())
}

#[tokio::test]
async fn client_secrets_support_basic_and_post_authentication() -> TestResult<()> {
    let app = Application::new().await?;
    for (method, use_basic) in [(None, true), (Some("client_secret_post"), false)] {
        let mut registration_body = json!({
            "redirect_uris": [TEST_REDIRECT_URI],
            "client_name": "Confidential client",
        });
        if let Some(method) = method {
            registration_body["token_endpoint_auth_method"] = json!(method);
        }
        let registration = app
            .request(json_request("POST", "/oauth/register", registration_body)?)
            .await?;
        assert_eq!(registration.status, StatusCode::CREATED);
        let client: Value = serde_json::from_str(&registration.body)?;
        let id = client["client_id"].as_str().ok_or("client id missing")?;
        let secret = client["client_secret"]
            .as_str()
            .ok_or("client secret missing")?;
        let fields = if use_basic {
            vec![
                ("grant_type", "unsupported"),
                ("resource", TEST_REST_RESOURCE),
            ]
        } else {
            vec![
                ("grant_type", "unsupported"),
                ("resource", TEST_REST_RESOURCE),
                ("client_id", id),
                ("client_secret", secret),
            ]
        };
        let mut request = form_request("/oauth/token", &fields, None, None)?;
        if use_basic {
            use base64::Engine;
            let credentials =
                base64::engine::general_purpose::STANDARD.encode(format!("{id}:{secret}"));
            request.headers_mut().insert(
                header::AUTHORIZATION,
                format!("Basic {credentials}").parse()?,
            );
        }
        let response = app.request(request).await?;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{}",
            response.body
        );
        assert!(response.body.contains("unsupported_grant_type"));
    }
    Ok(())
}

#[tokio::test]
async fn invalid_client_registration_is_rejected() -> TestResult<()> {
    let app = Application::new().await?;
    for body in [
        json!({"redirect_uris": ["http://not-loopback.example/callback"]}),
        json!({"redirect_uris": ["https://client.example/callback"], "token_endpoint_auth_method": "unknown"}),
        json!({"redirect_uris": ["https://client.example/callback"], "scope": "crumbs:unknown"}),
        json!({"redirect_uris": ["https://client.example/callback"], "scope": "  "}),
    ] {
        let response = app
            .request(json_request("POST", "/oauth/register", body)?)
            .await?;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{}",
            response.body
        );
    }
    Ok(())
}

#[tokio::test]
async fn readwrite_scope_is_shown_issued_and_preserved_by_refresh() -> TestResult<()> {
    let app = Application::new().await?;
    let registration = app
        .request(json_request(
            "POST",
            "/oauth/register",
            json!({
                "redirect_uris": [TEST_REDIRECT_URI],
                "token_endpoint_auth_method": "none",
                "grant_types": ["authorization_code", "refresh_token"],
                "scope": "crumbs:write crumbs:read crumbs:write"
            }),
        )?)
        .await?;
    assert_eq!(
        registration.status,
        StatusCode::CREATED,
        "{}",
        registration.body
    );
    let client: Value = serde_json::from_str(&registration.body)?;
    assert_eq!(client["scope"], "crumbs:read crumbs:write");
    let client_id = client["client_id"].as_str().ok_or("client id missing")?;
    let challenge = protocol::pkce_challenge(TEST_VERIFIER)?;
    let query = form_query(&[
        ("client_id", client_id),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("response_type", "code"),
        ("resource", TEST_REST_RESOURCE),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("scope", "crumbs:write crumbs:read"),
    ]);
    let page = app
        .request(get_request(&format!("/oauth/authorize?{query}"))?)
        .await?;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("Read the food library"));
    assert!(
        page.body
            .contains("Search the food library and view nutrition data")
    );
    assert!(page.body.contains("Create, edit, and delete custom foods"));

    let cookie = cookie_value(&page.headers).ok_or("login cookie missing")?;
    let request_id = page_value(&page.body, "request_id").ok_or("request id missing")?;
    let csrf_token = page_value(&page.body, "csrf_token").ok_or("CSRF token missing")?;
    let login = app
        .request(form_request(
            "/oauth/login",
            &[
                ("request_id", request_id),
                ("csrf_token", csrf_token),
                ("username", TEST_USERNAME),
                ("password", TEST_PASSWORD),
                ("decision", "approve"),
            ],
            Some(TEST_ISSUER),
            Some(&cookie),
        )?)
        .await?;
    assert_eq!(login.status, StatusCode::SEE_OTHER, "{}", login.body);
    let location = login
        .headers
        .get(header::LOCATION)
        .ok_or("authorization redirect missing")?
        .to_str()?;
    let redirect = url::Url::parse(location)?;
    let parameters: std::collections::HashMap<_, _> = redirect.query_pairs().into_owned().collect();
    let code = parameters.get("code").ok_or("authorization code missing")?;

    let token = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", client_id),
                ("code", code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_REST_RESOURCE),
                ("scope", "crumbs:read crumbs:write"),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(token.status, StatusCode::OK, "{}", token.body);
    let issued: Value = serde_json::from_str(&token.body)?;
    assert_eq!(issued["scope"], "crumbs:read crumbs:write");
    let refresh_token = issued["refresh_token"]
        .as_str()
        .ok_or("refresh token missing")?;

    let rotated = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("refresh_token", refresh_token),
                ("resource", TEST_REST_RESOURCE),
                ("scope", "crumbs:write crumbs:read"),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    let rotated: Value = serde_json::from_str(&rotated.body)?;
    assert_eq!(rotated["scope"], "crumbs:read crumbs:write");
    Ok(())
}

#[tokio::test]
async fn read_scope_cannot_be_escalated_during_code_exchange() -> TestResult<()> {
    let app = Application::new().await?;
    let code = "e".repeat(43);
    seed_authorization_code(
        &app.database,
        "read-only-code-client",
        TEST_REDIRECT_URI,
        TEST_REST_RESOURCE,
        &code,
        TEST_VERIFIER,
    )
    .await?;

    let response = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "read-only-code-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_REST_RESOURCE),
                ("scope", "crumbs:read crumbs:write"),
            ],
            None,
            None,
        )?)
        .await?;

    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert!(response.body.contains("invalid_scope"));

    let valid_retry = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "read-only-code-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_REST_RESOURCE),
                ("scope", "crumbs:read"),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(valid_retry.status, StatusCode::OK, "{}", valid_retry.body);
    let issued: Value = serde_json::from_str(&valid_retry.body)?;
    assert_eq!(issued["scope"], "crumbs:read");
    Ok(())
}

async fn seed_authorization_code(
    database: &Database,
    client_id: &str,
    redirect_uri: &str,
    resource: &str,
    code: &str,
    verifier: &str,
) -> TestResult<()> {
    let (user_id, password_hash) = test_user(database).await?;
    let now = protocol::now()?;
    let request_id = protocol::secret()?;
    let csrf_token = protocol::secret()?;
    let challenge = protocol::pkce_challenge(verifier)?;
    database
        .register_client(
            &OAuthClient {
                id: client_id.to_owned(),
                name: "Seeded client".to_owned(),
                redirect_uris: vec![redirect_uri.to_owned()],
                authentication: ClientAuthentication::Public,
                allow_refresh: true,
            },
            now,
        )
        .await?;
    let request_hash = protocol::digest(&request_id);
    let csrf_hash = protocol::digest(&csrf_token);
    database
        .begin_authorization(
            &PendingAuthorization {
                request_hash,
                csrf_hash,
                grant: AuthorizationGrant {
                    client_id: client_id.to_owned(),
                    redirect_uri: redirect_uri.to_owned(),
                    resource: resource.to_owned(),
                    scope: FoodScopes::Read,
                    state: None,
                    code_challenge: challenge.clone(),
                },
                expires_at: now + 600_000,
            },
            now,
        )
        .await?;
    database
        .approve_authorization(
            &Approval {
                request_hash,
                csrf_hash,
                user_id,
                password_hash,
                code_hash: protocol::digest(code),
                code_expires_at: now + 60_000,
            },
            now,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn code_exchange_binds_client_redirect_verifier_and_resource_and_is_one_time()
-> TestResult<()> {
    let app = Application::new().await?;
    let code = "c".repeat(43);
    seed_authorization_code(
        &app.database,
        "seeded-client",
        TEST_REDIRECT_URI,
        TEST_MCP_RESOURCE,
        &code,
        TEST_VERIFIER,
    )
    .await?;

    let wrong_verifier = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "seeded-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                (
                    "code_verifier",
                    "wrong-verifier-that-is-long-enough-to-fail",
                ),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(wrong_verifier.status, StatusCode::BAD_REQUEST);
    assert!(wrong_verifier.body.contains("invalid_grant"));

    let wrong_redirect = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "seeded-client"),
                ("code", &code),
                ("redirect_uri", "https://other.example/callback"),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(wrong_redirect.status, StatusCode::BAD_REQUEST);
    assert!(wrong_redirect.body.contains("invalid_grant"));

    let wrong_resource = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "seeded-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_REST_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(wrong_resource.status, StatusCode::BAD_REQUEST);
    assert!(wrong_resource.body.contains("invalid_grant"));

    let wrong_client = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "another-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(wrong_client.status, StatusCode::UNAUTHORIZED);
    assert!(wrong_client.body.contains("invalid_client"));

    let missing_resource = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "seeded-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(missing_resource.status, StatusCode::BAD_REQUEST);
    assert!(missing_resource.body.contains("invalid_target"));

    let valid = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "seeded-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(valid.status, StatusCode::OK, "{}", valid.body);
    let replay = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "seeded-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_MCP_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(replay.status, StatusCode::BAD_REQUEST);
    assert!(replay.body.contains("invalid_grant"));
    Ok(())
}

#[tokio::test]
async fn refresh_replay_revokes_the_family_and_revocation_rejects_other_clients() -> TestResult<()>
{
    let app = Application::new().await?;
    let code = "d".repeat(43);
    seed_authorization_code(
        &app.database,
        "refresh-client",
        TEST_REDIRECT_URI,
        TEST_REST_RESOURCE,
        &code,
        TEST_VERIFIER,
    )
    .await?;
    let exchange = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "refresh-client"),
                ("code", &code),
                ("redirect_uri", TEST_REDIRECT_URI),
                ("code_verifier", TEST_VERIFIER),
                ("resource", TEST_REST_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(exchange.status, StatusCode::OK, "{}", exchange.body);
    let issued: Value = serde_json::from_str(&exchange.body)?;
    let old_refresh = issued["refresh_token"]
        .as_str()
        .ok_or("refresh token missing")?;
    let rotated = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", "refresh-client"),
                ("refresh_token", old_refresh),
                ("resource", TEST_REST_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    let rotated_tokens: Value = serde_json::from_str(&rotated.body)?;
    let new_access = rotated_tokens["access_token"]
        .as_str()
        .ok_or("access token missing")?;

    let replay = app
        .request(form_request(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", "refresh-client"),
                ("refresh_token", old_refresh),
                ("resource", TEST_REST_RESOURCE),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(replay.status, StatusCode::BAD_REQUEST);
    assert!(replay.body.contains("invalid_grant"));
    let revoked = app
        .request(bearer_request("/foods/search?query=chicken", new_access)?)
        .await?;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED);

    let foreign_client = app
        .request(form_request(
            "/oauth/revoke",
            &[
                ("token", TEST_REST_ACCESS_TOKEN),
                ("client_id", "refresh-client"),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(foreign_client.status, StatusCode::OK);
    let foreign_token_still_valid = app
        .request(bearer_request(
            "/foods/search?query=chicken",
            TEST_REST_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(foreign_token_still_valid.status, StatusCode::OK);

    let revocation = app
        .request(form_request(
            "/oauth/revoke",
            &[
                ("token", TEST_REST_ACCESS_TOKEN),
                ("client_id", "test-rest-client"),
            ],
            None,
            None,
        )?)
        .await?;
    assert_eq!(revocation.status, StatusCode::OK);
    let revoked_token = app
        .request(bearer_request(
            "/foods/search?query=chicken",
            TEST_REST_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(revoked_token.status, StatusCode::UNAUTHORIZED);
    Ok(())
}
