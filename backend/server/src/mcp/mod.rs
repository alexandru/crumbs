//! Shared MCP transport and tool dispatch.

mod authorization;
mod server;
mod tools;

pub(crate) use authorization::McpAuthorization;
pub(crate) use server::{config, service};
pub(crate) use tools::ToolRegistration;
