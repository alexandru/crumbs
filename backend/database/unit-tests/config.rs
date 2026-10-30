use super::{DatabaseConfig, DatabasePath};

#[test]
fn deserializes_database_path() -> Result<(), serde_json::Error> {
    let config: DatabaseConfig = serde_json::from_str(r#"{"path": "data/crumbs.db"}"#)?;

    assert_eq!(config.path, DatabasePath::from("data/crumbs.db"));
    Ok(())
}

#[test]
fn rejects_missing_path() {
    assert!(serde_json::from_str::<DatabaseConfig>(r#"{}"#).is_err());
}

#[test]
fn rejects_wrong_path_type() {
    assert!(serde_json::from_str::<DatabaseConfig>(r#"{"path": ["data"]}"#).is_err());
}

#[test]
fn rejects_unknown_key() {
    assert!(
        serde_json::from_str::<DatabaseConfig>(r#"{"path": "data/crumbs.db", "mode": "wal"}"#)
            .is_err()
    );
}
