use super::*;

#[test]
fn credential_hash_exposes_bytes_and_redacts_debug() {
    let bytes = [42; 32];
    let hash = CredentialHash::new(bytes);
    assert_eq!(hash.as_bytes(), &bytes);
    assert_eq!(format!("{hash:?}"), "CredentialHash([REDACTED])");
}

#[test]
fn authorization_grant_redacts_client_state_in_debug() {
    let grant = AuthorizationGrant {
        client_id: "client".to_owned(),
        redirect_uri: "https://client.example/callback".to_owned(),
        resource: "https://api.example".to_owned(),
        scope: "read".to_owned(),
        state: Some("client-secret-state".to_owned()),
        code_challenge: "challenge".to_owned(),
    };
    let debug = format!("{grant:?}");
    assert!(!debug.contains("client-secret-state"));
    assert!(debug.contains("[REDACTED]"));
}
