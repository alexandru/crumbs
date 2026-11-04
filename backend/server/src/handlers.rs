//! HTTP handlers for the greeting and food search routes.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use crumbs_database::{FoodReader, SearchError};
use serde::Serialize;

use crate::search::{self, SearchRequest};

#[derive(Debug, Serialize)]
pub(super) struct Greeting {
    greeting: &'static str,
}

pub(super) async fn hello() -> Json<Greeting> {
    Json(Greeting {
        greeting: "Hello, world!",
    })
}

pub(super) async fn search_foods<S: FoodReader + Send + 'static>(
    State(reader): State<Arc<S>>,
    request: Result<Query<SearchRequest>, QueryRejection>,
) -> Response {
    let Query(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return failure(StatusCode::BAD_REQUEST, rejection.to_string());
        }
    };

    match search::execute(reader.as_ref(), request).await {
        Ok(response) => Json(response).into_response(),
        Err(SearchError::InvalidQuery(diagnostic)) => failure(StatusCode::BAD_REQUEST, diagnostic),
        Err(SearchError::Storage(_)) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "food search failed".to_owned(),
        ),
    }
}

#[derive(Debug, Serialize)]
struct SearchFailure {
    error: String,
}

fn failure(status: StatusCode, error: String) -> Response {
    (status, Json(SearchFailure { error })).into_response()
}

#[cfg(test)]
#[path = "../unit-tests/handlers.rs"]
mod tests;
