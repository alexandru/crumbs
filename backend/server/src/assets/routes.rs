use axum::Router;

use super::handlers;

pub(crate) fn routes() -> Router {
    Router::new()
        .route(
            "/assets/main.css",
            axum::routing::get(|| async { handlers::stylesheet() }),
        )
        .route(
            "/assets/login.css",
            axum::routing::get(|| async { handlers::stylesheet() }),
        )
}

#[cfg(test)]
#[path = "../../unit-tests/assets/routes.rs"]
mod tests;
