use serde::Deserialize;

use crate::oauth::OAuthConfig;

pub type HttpPort = u16;
pub type HttpBindAddress = std::net::IpAddr;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    #[serde(default = "loopback")]
    pub bind: HttpBindAddress,
    pub port: HttpPort,
    pub oauth: OAuthConfig,
}

fn loopback() -> HttpBindAddress {
    std::net::Ipv4Addr::LOCALHOST.into()
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
