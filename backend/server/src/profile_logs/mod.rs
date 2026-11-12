//! REST and MCP management of profile logs.

mod handlers;
mod mcp;
mod routes;

pub(crate) use mcp::mcp_tools;
pub(crate) use routes::routes;

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/mod.rs"]
mod tests;
