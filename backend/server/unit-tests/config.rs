use std::net::{IpAddr, Ipv4Addr};

use super::HttpConfig;

#[test]
fn deserializes_http_settings_and_defaults_to_loopback() -> Result<(), Box<dyn std::error::Error>> {
    let config: HttpConfig =
        serde_json::from_str(r#"{"port":8080,"oauth":{"issuer":"https://crumbs.example.com"}}"#)?;

    assert_eq!(config.bind, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(config.port, 8080);
    assert_eq!(config.oauth.issuer.as_str(), "https://crumbs.example.com");
    Ok(())
}

#[test]
fn deserializes_explicit_bind_address() -> Result<(), Box<dyn std::error::Error>> {
    let config: HttpConfig = serde_json::from_str(
        r#"{"bind":"0.0.0.0","port":8080,"oauth":{"issuer":"https://crumbs.example.com"}}"#,
    )?;

    assert_eq!(config.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    Ok(())
}

#[test]
fn rejects_missing_port() {
    assert!(
        serde_json::from_str::<HttpConfig>(r#"{"oauth":{"issuer":"https://crumbs.example.com"}}"#)
            .is_err()
    );
}

#[test]
fn rejects_missing_oauth_issuer() {
    assert!(serde_json::from_str::<HttpConfig>(r#"{"port":8080,"oauth":{}}"#).is_err());
}

#[test]
fn rejects_wrong_port_type() {
    assert!(
        serde_json::from_str::<HttpConfig>(
            r#"{"port":true,"oauth":{"issuer":"https://crumbs.example.com"}}"#
        )
        .is_err()
    );
}

#[test]
fn rejects_out_of_range_port() {
    assert!(
        serde_json::from_str::<HttpConfig>(
            r#"{"port":70000,"oauth":{"issuer":"https://crumbs.example.com"}}"#
        )
        .is_err()
    );
}

#[test]
fn rejects_unknown_key() {
    assert!(
        serde_json::from_str::<HttpConfig>(
            r#"{"port":8080,"host":"127.0.0.1","oauth":{"issuer":"https://crumbs.example.com"}}"#
        )
        .is_err()
    );
}
