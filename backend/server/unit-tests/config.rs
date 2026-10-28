use super::HttpConfig;

#[test]
fn deserializes_http_port() -> Result<(), serde_json::Error> {
    let config: HttpConfig = serde_json::from_str(r#"{"port": 8080}"#)?;

    assert_eq!(config, HttpConfig { port: 8080 });
    Ok(())
}

#[test]
fn rejects_missing_port() {
    assert!(serde_json::from_str::<HttpConfig>(r#"{}"#).is_err());
}

#[test]
fn rejects_wrong_port_type() {
    assert!(serde_json::from_str::<HttpConfig>(r#"{"port": true}"#).is_err());
}

#[test]
fn rejects_out_of_range_port() {
    assert!(serde_json::from_str::<HttpConfig>(r#"{"port": 70000}"#).is_err());
}

#[test]
fn rejects_unknown_key() {
    assert!(serde_json::from_str::<HttpConfig>(r#"{"port": 8080, "host": "127.0.0.1"}"#).is_err());
}
