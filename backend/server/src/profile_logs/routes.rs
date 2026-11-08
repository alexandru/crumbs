use std::sync::Arc;

use axum::Router;
use crumbs_database::profile_logs::ProfileLogs;

use super::handlers;

pub(crate) fn routes<S>(store: Arc<S>) -> Router
where
    S: ProfileLogs + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/profiles/{profile_id}/logs",
            axum::routing::get(handlers::list::<S>).post(handlers::create::<S>),
        )
        .route(
            "/profiles/{profile_id}/logs/{log_id}",
            axum::routing::get(handlers::get::<S>)
                .patch(handlers::patch::<S>)
                .delete(handlers::delete::<S>),
        )
        .with_state(store)
}
