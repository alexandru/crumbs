use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use crumbs_database::oauth::CredentialHash;
use std::time::Duration;

use super::*;

fn redirect_matches(registered: &str, requested: &str) -> bool {
    let Ok(registered) = redirect_uri(registered) else {
        return false;
    };
    let Ok(requested) = redirect_uri(requested) else {
        return false;
    };

    redirect_matches_parsed(&registered, &requested)
}

#[test]
fn secret_is_a_32_byte_url_safe_credential() -> Result<(), ProtocolError> {
    let value = secret()?;

    assert_eq!(value.len(), 43);
    assert!(
        value
            .bytes()
            .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') })
    );
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(value.as_bytes())
            .map(|bytes| bytes.len()),
        Ok(32)
    );
    Ok(())
}

#[test]
fn digest_uses_sha256() {
    let expected = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];

    assert_eq!(digest("abc"), CredentialHash::new(expected));
}

#[test]
fn timestamp_is_unix_epoch_milliseconds() {
    assert!(now().is_ok_and(|timestamp| timestamp >= 0));
}

#[test]
fn expiry_addition_checks_the_timestamp_boundary() {
    assert!(
        expires_after(i64::MAX - 1, Duration::from_millis(1))
            .is_ok_and(|timestamp| timestamp == i64::MAX)
    );
    assert!(expires_after(i64::MAX, Duration::from_millis(1)).is_err());
    assert!(expires_after(0, Duration::from_secs(u64::MAX)).is_err());
}

#[test]
fn pkce_challenge_matches_rfc_7636_vector() {
    let challenge = pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");

    assert!(challenge.is_ok_and(|value| value == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
}

#[test]
fn pkce_verifier_must_be_unreserved_ascii_and_in_range() {
    assert!(pkce_challenge(&"a".repeat(42)).is_err());
    assert!(pkce_challenge(&"a".repeat(129)).is_err());
    assert!(pkce_challenge(&format!("{} ", "a".repeat(42))).is_err());
    assert!(pkce_challenge(&format!("{}é", "a".repeat(41))).is_err());
}

#[test]
fn challenge_requires_canonical_32_byte_base64url_without_padding() {
    let valid = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    assert!(valid_challenge(valid));
    assert!(!valid_challenge(
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM="
    ));
    assert!(!valid_challenge(
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-c!"
    ));
    assert!(!valid_challenge(&format!("{}B", "A".repeat(42))));
}

#[test]
fn redirects_accept_https_and_loopback_ip_http_only() {
    assert!(redirect_uri("https://client.example/callback").is_ok());
    assert!(redirect_uri("http://127.0.0.1:43827/callback").is_ok());
    assert!(redirect_uri("http://127.23.4.5/callback").is_ok());
    assert!(redirect_uri("http://[::1]:43827/callback").is_ok());
}

#[test]
fn redirects_reject_unsafe_or_ambiguous_uris() {
    for value in [
        "http://example.test/callback",
        "http://localhost/callback",
        "https://user@example.test/callback",
        "https://@example.test/callback",
        "https://example.test/callback#fragment",
        "https://*.example.test/callback",
        "https:///callback",
        "http://127.0.0.1:0/callback",
        "https://example.test/callback with spaces",
        "https://example.test\\callback",
        "custom://client/callback",
        "https://example.test/callback?code=existing",
        "https://example.test/callback?st%61te=existing",
        "https://example.test/callback?error_description=existing",
    ] {
        assert!(redirect_uri(value).is_err(), "accepted {value}");
    }

    let over_limit = format!("https://example.test/callback?x={}", "x".repeat(2048));
    assert!(redirect_uri(&over_limit).is_err());
}

#[test]
fn redirect_matching_is_exact_except_for_loopback_port() {
    assert!(redirect_matches(
        "https://client.example/callback?tenant=one",
        "https://client.example/callback?tenant=one"
    ));
    assert!(redirect_matches(
        "http://127.0.0.1:1234/callback?tenant=one",
        "http://127.0.0.1:4321/callback?tenant=one"
    ));
    assert!(redirect_matches(
        "http://[::1]/callback",
        "http://[::1]:4321/callback"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1:1234/callback",
        "http://127.0.0.1:4321/callback/"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1:1234/a/../callback",
        "http://127.0.0.1:4321/callback"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1:1234/callback",
        "http://127.0.0.1:4321/%63allback"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1:1234/callback?tenant=one",
        "http://127.0.0.1:4321/callback?tenant=two"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1:1234/callback",
        "http://127.0.0.2:4321/callback"
    ));
    assert!(!redirect_matches(
        "https://client.example:1234/callback",
        "https://client.example:4321/callback"
    ));
    assert!(!redirect_matches(
        "https://client.example/callback/../callback",
        "https://client.example/callback"
    ));
    assert!(!redirect_matches("not a URI", "not a URI"));
}

#[test]
fn redirect_response_encodes_values_and_preserves_the_redirect_uri() -> Result<(), ProtocolError> {
    let original = "https://client.example/a/../callback?tenant=one%20two";
    let response = redirect_response(
        original,
        Some("state & value"),
        "https://issuer.example/",
        Ok("code+/="),
    );

    let response = response?;

    assert!(response.starts_with(original));
    assert!(response.contains("&code=code%2B%2F%3D"));
    assert!(response.contains("state=state+%26+value"));
    assert!(response.contains("iss=https%3A%2F%2Fissuer.example%2F"));

    let error_response = redirect_response(
        "https://client.example/callback",
        None,
        "https://issuer.example",
        Err("access_denied"),
    );
    assert!(error_response.is_ok_and(|uri| uri.contains("?error=access_denied&iss=")));
    Ok(())
}

#[test]
fn bearer_accepts_one_rfc_6750_header() -> Result<(), axum::http::header::InvalidHeaderValue> {
    for value in ["Bearer abcDEF012-._~+/=", "bEaReR   token=="] {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_bytes(value.as_bytes())?);
        assert!(bearer(&headers).is_some());
    }
    Ok(())
}

#[test]
fn bearer_rejects_invalid_schemes_tokens_and_duplicates() {
    for value in [
        "Basic abc",
        "Bearer",
        "Bearer ",
        "Bearer ,",
        "Bearer abc,def",
        "Bearer abc def",
        "Bearer abc\tdef",
        "Bearer =",
    ] {
        let mut headers = HeaderMap::new();
        if let Ok(header) = HeaderValue::from_bytes(value.as_bytes()) {
            headers.insert(AUTHORIZATION, header);
            assert_eq!(bearer(&headers), None, "accepted {value:?}");
        }
    }

    let mut duplicate_headers = HeaderMap::new();
    duplicate_headers.append(AUTHORIZATION, HeaderValue::from_static("Bearer first"));
    duplicate_headers.append(AUTHORIZATION, HeaderValue::from_static("Bearer second"));
    assert_eq!(bearer(&duplicate_headers), None);
}

#[test]
fn bearer_enforces_token_length_limit() -> Result<(), axum::http::header::InvalidHeaderValue> {
    let mut headers = HeaderMap::new();
    let token = "a".repeat(256);
    let valid_header = format!("Bearer {token}");
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_bytes(valid_header.as_bytes())?,
    );
    assert_eq!(bearer(&headers).map(str::len), Some(256));

    let token = "a".repeat(257);
    let invalid_header = format!("Bearer {token}");
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_bytes(invalid_header.as_bytes())?,
    );
    assert_eq!(bearer(&headers), None);
    Ok(())
}
