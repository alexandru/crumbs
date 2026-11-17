use std::sync::Arc;

use axum::Router;
use crumbs_database::daily_status::DailyStatusStore;

use super::handlers;

pub(crate) fn routes<S>(store: Arc<S>) -> Router
where
    S: DailyStatusStore + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/profiles/{profile_id}/daily-status",
            axum::routing::get(handlers::get::<S>),
        )
        .with_state(store)
}
