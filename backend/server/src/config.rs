//! HTTP listener configuration owned by the server crate.

use serde::Deserialize;

use crate::oauth::OAuthConfig;

pub type HttpPort = u16;
pub type HttpBindAddress = std::net::IpAddr;

/// Settings for the HTTP listener.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    /// Interface to bind. Defaults to loopback for a local TLS reverse proxy.
    #[serde(default = "loopback")]
    pub bind: HttpBindAddress,
    /// TCP port to listen on.
    ///
    /// Port `0` lets the operating system choose an available port.
    pub port: HttpPort,
    /// Public OAuth issuer used for discovery and redirect validation.
    pub oauth: OAuthConfig,
}

fn loopback() -> HttpBindAddress {
    std::net::Ipv4Addr::LOCALHOST.into()
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
