//! HTTP and MCP food search request handling.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use crumbs_database::{Database, FoodSearchResult, SearchError, SearchLimit, SearchQuery};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct SearchRequest {
    pub(crate) query: SearchQuery,
    #[serde(default)]
    pub(crate) limit: SearchLimit,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchResponse {
    pub(crate) foods: Vec<FoodSearchResult>,
}

pub(crate) async fn execute(
    database: &Database,
    request: SearchRequest,
) -> Result<SearchResponse, SearchError> {
    let foods = database.search_foods(&request.query, request.limit).await?;
    Ok(SearchResponse { foods })
}

pub(crate) async fn handler(
    State(database): State<Arc<Database>>,
    request: Result<Query<SearchRequest>, QueryRejection>,
) -> Response {
    let Query(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return failure(StatusCode::BAD_REQUEST, rejection.to_string());
        }
    };

    match execute(&database, request).await {
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
#[path = "../unit-tests/search.rs"]
mod tests;
