//! HTTP handling for food search.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use crumbs_database::foods::{FoodSearch, SearchError};
use serde::Serialize;

use super::SearchRequest;

pub(super) async fn search_foods<S: FoodSearch + Send + 'static>(
    State(reader): State<Arc<S>>,
    request: Result<Query<SearchRequest>, QueryRejection>,
) -> Response {
    let Query(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return failure(StatusCode::BAD_REQUEST, rejection.to_string());
        }
    };

    match super::execute(reader.as_ref(), request).await {
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
#[path = "../../unit-tests/search/handlers.rs"]
mod tests;
