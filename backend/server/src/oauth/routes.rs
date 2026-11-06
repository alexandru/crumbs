use std::sync::Arc;

use axum::Router;
use crumbs_database::oauth::OAuthStore;
use crumbs_database::users::Users;

use super::{AuthorizationServer, authorization, tokens};

pub(crate) fn routes<S>(server: Arc<AuthorizationServer<S>>) -> Router
where
    S: Users + OAuthStore + Send + 'static,
{
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(super::metadata::<S>),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            axum::routing::get(super::mcp_metadata::<S>),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            axum::routing::get(super::rest_metadata::<S>),
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
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&server),
            super::throttle::<S>,
        ))
        .with_state(server)
}
