mod handlers;
mod mcp;
mod routes;

pub(crate) use mcp::mcp_tools;
pub(crate) use routes::routes;

#[cfg(test)]
#[path = "../../unit-tests/daily_status/mod.rs"]
mod tests;
