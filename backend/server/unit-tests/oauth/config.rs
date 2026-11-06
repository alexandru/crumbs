use super::{IssuerUrl, OAuthConfig};

#[test]
fn permits_https_issuer_and_loopback_http_issuer() -> Result<(), super::InvalidIssuerUrl> {
    assert_eq!(
        IssuerUrl::new("https://crumbs.example.com")?.as_str(),
        "https://crumbs.example.com"
    );
    assert_eq!(
        IssuerUrl::new("http://127.0.0.1:8080")?.as_str(),
        "http://127.0.0.1:8080"
    );
    Ok(())
}

#[test]
fn rejects_non_loopback_http_and_non_origin_issuers() {
    for issuer in [
        "http://crumbs.example.com",
        "https://crumbs.example.com/path",
        "https://user@crumbs.example.com",
        "https://crumbs.example.com?query=1",
    ] {
        assert!(
            IssuerUrl::new(issuer).is_err(),
            "unexpectedly accepted issuer {issuer:?}"
        );
    }
}

#[test]
fn oauth_config_uses_the_validated_issuer_type() -> Result<(), super::InvalidIssuerUrl> {
    let config = OAuthConfig {
        issuer: IssuerUrl::new("https://crumbs.example.com")?,
    };

    assert_eq!(config.issuer.as_str(), "https://crumbs.example.com");
    Ok(())
}
