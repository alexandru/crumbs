//! Food search across the common, branded, and custom datasets.

mod handlers;
mod mcp;
mod routes;

use crumbs_database::foods::{FoodLimit, FoodSearch, SearchError, SearchQuery, SearchResults};
use serde::Deserialize;

pub(crate) use mcp::mcp_tools;
pub(crate) use routes::routes;

#[derive(Debug, Deserialize)]
struct SearchRequest {
    query: SearchQuery,
    #[serde(default)]
    limit: FoodLimit,
}

async fn execute<S: FoodSearch + ?Sized>(
    reader: &S,
    request: SearchRequest,
) -> Result<SearchResults, SearchError<S::Error>> {
    reader.search_foods(&request.query, request.limit).await
}

#[cfg(test)]
#[path = "../../unit-tests/search/mod.rs"]
mod tests;
