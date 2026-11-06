use std::sync::Arc;

use axum::Router;
use crumbs_database::FoodReader;

use super::handlers;

pub(crate) fn routes<S>(reader: Arc<S>) -> Router
where
    S: FoodReader + Send + 'static,
{
    Router::new()
        .route(
            "/foods/search",
            axum::routing::get(handlers::search_foods::<S>),
        )
        .with_state(reader)
}
