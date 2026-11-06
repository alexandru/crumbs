use axum::Router;

use super::handlers;

pub(crate) fn routes() -> Router {
    Router::new().route("/", axum::routing::get(handlers::hello))
}
