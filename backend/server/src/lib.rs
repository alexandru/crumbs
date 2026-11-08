//! HTTP server for Crumbs.

#[cfg(not(unix))]
compile_error!("crumbs-server currently supports Unix targets only");

mod assets;
mod config;
mod custom_foods;
mod home;
mod mcp;
mod oauth;
mod profile_logs;
mod profiles;
mod routes;
mod runtime;
mod search;

pub use config::{HttpBindAddress, HttpConfig, HttpPort};
pub use oauth::{InitError as OAuthInitError, InvalidIssuerUrl, IssuerUrl, OAuthConfig};
pub use runtime::{Error, start};

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
