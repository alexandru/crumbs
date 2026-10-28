//! HTTP listener configuration owned by the server crate.

use serde::Deserialize;

pub type HttpPort = u16;

/// Settings for the HTTP listener.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    /// TCP port to listen on.
    ///
    /// Port `0` lets the operating system choose an available port.
    pub port: HttpPort,
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
