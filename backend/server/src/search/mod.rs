//! Food search across the common, branded, and custom datasets.

mod handlers;
mod mcp;
mod routes;

use crumbs_database::foods::{FoodLimit, SearchQuery};
use serde::Deserialize;

pub(crate) use mcp::mcp_tools;
pub(crate) use routes::routes;

#[derive(Debug, Deserialize)]
struct SearchRequest {
    query: SearchQuery,
    #[serde(default)]
    limit: FoodLimit,
}

#[cfg(test)]
#[path = "../../unit-tests/search/mod.rs"]
mod tests;
