use serde::Deserialize;
use url::Url;

/// OAuth authorization-server settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OAuthConfig {
    /// Externally reachable origin, using HTTPS except for loopback development.
    pub issuer: IssuerUrl,
}

/// A canonical public origin without credentials, query, fragment, or a path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct IssuerUrl(Url);

impl IssuerUrl {
    /// Parses an HTTPS origin, or an HTTP loopback IP origin for development.
    ///
    /// # Errors
    /// Returns an error for an invalid URL, non-origin URL, or insecure remote origin.
    pub fn new(value: &str) -> Result<Self, InvalidIssuerUrl> {
        let url = Url::parse(value).map_err(|_| InvalidIssuerUrl)?;
        let loopback = match url.host() {
            Some(url::Host::Ipv4(address)) => address.is_loopback(),
            Some(url::Host::Ipv6(address)) => address.is_loopback(),
            _ => false,
        };
        if url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || url.port() == Some(0)
        {
            return Err(InvalidIssuerUrl);
        }
        Ok(Self(url))
    }

    /// Returns the issuer identifier without a trailing slash.
    pub fn as_str(&self) -> &str {
        self.0.as_str().trim_end_matches('/')
    }

    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.as_str())
    }

    pub(crate) fn secure(&self) -> bool {
        self.0.scheme() == "https"
    }
}

impl TryFrom<String> for IssuerUrl {
    type Error = InvalidIssuerUrl;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

/// The OAuth issuer is not a permitted public origin.
#[derive(Debug, thiserror::Error)]
#[error(
    "OAuth issuer must be an HTTPS origin, or an HTTP loopback IP origin, without credentials, path, query, or fragment"
)]
pub struct InvalidIssuerUrl;

#[cfg(test)]
#[path = "../../unit-tests/oauth/config.rs"]
mod tests;
