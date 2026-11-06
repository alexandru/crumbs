//! Shared food search request and response types.

use crumbs_database::{FoodReader, FoodSearchResults, SearchError, SearchLimit, SearchQuery};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct SearchRequest {
    pub(crate) query: SearchQuery,
    #[serde(default)]
    pub(crate) limit: SearchLimit,
}

pub(crate) async fn execute<S: FoodReader + ?Sized>(
    reader: &S,
    request: SearchRequest,
) -> Result<FoodSearchResults, SearchError<S::Error>> {
    reader.search_foods(&request.query, request.limit).await
}
