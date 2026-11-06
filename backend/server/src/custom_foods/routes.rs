use std::sync::Arc;

use axum::Router;
use crumbs_database::foods::CustomFoodStore;

use super::handlers;

pub(crate) fn routes<S>(store: Arc<S>) -> Router
where
    S: CustomFoodStore + Send + 'static,
{
    Router::new()
        .route(
            "/foods/custom",
            axum::routing::get(handlers::list::<S>).post(handlers::create::<S>),
        )
        .route(
            "/foods/custom/{id}",
            axum::routing::get(handlers::get::<S>)
                .put(handlers::update::<S>)
                .delete(handlers::delete::<S>),
        )
        .with_state(store)
}

#[cfg(test)]
#[path = "../../unit-tests/custom_foods/routes.rs"]
mod tests;
