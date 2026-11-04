//! Shared food search request and response types.

use crumbs_database::{FoodReader, FoodSearchResult, SearchError, SearchLimit, SearchQuery};
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

pub(crate) async fn execute<S: FoodReader + ?Sized>(
    reader: &S,
    request: SearchRequest,
) -> Result<SearchResponse, SearchError<S::Error>> {
    let foods = reader.search_foods(&request.query, request.limit).await?;
    Ok(SearchResponse { foods })
}
