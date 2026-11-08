use std::sync::Arc;

use axum::Router;
use crumbs_database::profiles::Profiles;

use super::handlers;

pub(crate) fn routes<S>(store: Arc<S>) -> Router
where
    S: Profiles + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/profiles",
            axum::routing::get(handlers::list::<S>).post(handlers::create::<S>),
        )
        .route(
            "/profiles/{profile_id}",
            axum::routing::get(handlers::get::<S>).patch(handlers::patch::<S>),
        )
        .with_state(store)
}
