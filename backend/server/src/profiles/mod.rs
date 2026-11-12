//! REST and MCP management of account-owned profiles.

mod handlers;
mod mcp;
mod routes;

pub(crate) use mcp::mcp_tools;
pub(crate) use routes::routes;

#[cfg(test)]
#[path = "../../unit-tests/profiles/mod.rs"]
mod tests;
