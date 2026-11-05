use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, header::AUTHORIZATION};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use crumbs_database::oauth::{AuthTimestamp, CredentialHash, PkceChallenge, RedirectUri};
use sha2::{Digest, Sha256};
use url::{Host, Url};

const MAX_REDIRECT_URI_BYTES: usize = 2048;
const MAX_BEARER_TOKEN_BYTES: usize = 256;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProtocolError {
    #[error("failed to generate an OAuth credential")]
    Entropy,
    #[error("system clock returned an invalid OAuth timestamp")]
    Clock,
    #[error("invalid OAuth protocol input")]
    InvalidRequest,
}

pub(super) fn secret() -> Result<String, ProtocolError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| ProtocolError::Entropy)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn digest(value: &str) -> CredentialHash {
    let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    CredentialHash::new(digest)
}

pub(crate) fn now() -> Result<AuthTimestamp, ProtocolError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ProtocolError::Clock)?;
    elapsed
        .as_millis()
        .try_into()
        .map_err(|_| ProtocolError::Clock)
}

pub(super) fn expires_after(
    now: AuthTimestamp,
    lifetime: Duration,
) -> Result<AuthTimestamp, ProtocolError> {
    let lifetime_ms: AuthTimestamp = lifetime
        .as_millis()
        .try_into()
        .map_err(|_| ProtocolError::Clock)?;
    now.checked_add(lifetime_ms).ok_or(ProtocolError::Clock)
}

pub(super) fn pkce_challenge(verifier: &str) -> Result<PkceChallenge, ProtocolError> {
    let bytes = verifier.as_bytes();
    if !(43..=128).contains(&bytes.len()) || !bytes.iter().copied().all(is_pkce_unreserved) {
        return Err(ProtocolError::InvalidRequest);
    }

    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(bytes)))
}

pub(super) fn valid_challenge(value: &str) -> bool {
    if value.len() != 43 {
        return false;
    }

    let Ok(decoded) = URL_SAFE_NO_PAD.decode(value) else {
        return false;
    };
    decoded.len() == 32 && URL_SAFE_NO_PAD.encode(decoded) == value
}

pub(super) fn redirect_uri(value: &str) -> Result<RedirectUri, ProtocolError> {
    validated_redirect(value)?;
    Ok(value.to_owned())
}

pub(super) fn redirect_matches(registered: &str, requested: &str) -> bool {
    let Ok(registered_url) = validated_redirect(registered) else {
        return false;
    };
    let Ok(requested_url) = validated_redirect(requested) else {
        return false;
    };

    if registered == requested {
        return true;
    }

    if registered_url.scheme() != "http"
        || requested_url.scheme() != "http"
        || !is_loopback_ip(&registered_url)
        || !is_loopback_ip(&requested_url)
    {
        return false;
    }

    let (
        Some((registered_scheme, registered_authority, registered_suffix)),
        Some((requested_scheme, requested_authority, requested_suffix)),
    ) = (raw_uri_parts(registered), raw_uri_parts(requested))
    else {
        return false;
    };

    if registered_scheme != requested_scheme || registered_suffix != requested_suffix {
        return false;
    }

    let Ok((registered_host, _)) = authority_without_port(registered_authority) else {
        return false;
    };
    let Ok((requested_host, _)) = authority_without_port(requested_authority) else {
        return false;
    };
    registered_host == requested_host
}

pub(super) fn redirect_response(
    uri: &str,
    state: Option<&str>,
    issuer: &str,
    result: Result<&str, &str>,
) -> Result<String, ProtocolError> {
    validated_redirect(uri)?;

    let mut encoded_query = url::form_urlencoded::Serializer::new(String::new());
    match result {
        Ok(code) => {
            encoded_query.append_pair("code", code);
        }
        Err(error) => {
            encoded_query.append_pair("error", error);
        }
    }
    if let Some(state) = state {
        encoded_query.append_pair("state", state);
    }
    encoded_query.append_pair("iss", issuer);
    let encoded_query = encoded_query.finish();

    let response_capacity = uri
        .len()
        .checked_add(encoded_query.len())
        .and_then(|capacity| capacity.checked_add(1))
        .ok_or(ProtocolError::InvalidRequest)?;
    let mut response_uri = String::new();
    response_uri
        .try_reserve(response_capacity)
        .map_err(|_| ProtocolError::InvalidRequest)?;
    response_uri.push_str(uri);
    if uri.contains('?') {
        if !uri.ends_with('?') && !uri.ends_with('&') {
            response_uri.push('&');
        }
    } else {
        response_uri.push('?');
    }
    response_uri.push_str(&encoded_query);
    Ok(response_uri)
}

pub(super) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let header = values.next()?;
    if values.next().is_some() {
        return None;
    }

    let value = header.to_str().ok()?;
    let bytes = value.as_bytes();
    if !bytes.get(..6)?.eq_ignore_ascii_case(b"Bearer") {
        return None;
    }

    let mut token_start = 6;
    while bytes.get(token_start) == Some(&b' ') {
        token_start += 1;
    }
    if token_start == 6 {
        return None;
    }

    let token = bytes.get(token_start..)?;
    if token.is_empty() || token.len() > MAX_BEARER_TOKEN_BYTES {
        return None;
    }

    let first_padding = token.iter().position(|byte| *byte == b'=');
    let (body, padding) = match first_padding {
        Some(index) => token.split_at_checked(index)?,
        None => (token, &[][..]),
    };
    if body.is_empty()
        || !body.iter().copied().all(is_bearer_body_byte)
        || !padding.iter().all(|byte| *byte == b'=')
    {
        return None;
    }

    value.get(token_start..)
}

fn validated_redirect(value: &str) -> Result<Url, ProtocolError> {
    if value.len() > MAX_REDIRECT_URI_BYTES
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ' || byte == b'\\')
    {
        return Err(ProtocolError::InvalidRequest);
    }

    let (raw_scheme, raw_authority, _) =
        raw_uri_parts(value).ok_or(ProtocolError::InvalidRequest)?;
    if raw_scheme.is_empty() || raw_authority.is_empty() || raw_authority.contains('@') {
        return Err(ProtocolError::InvalidRequest);
    }

    let url = Url::parse(value).map_err(|_| ProtocolError::InvalidRequest)?;
    if url.host().is_none()
        || url.fragment().is_some()
        || url.host_str().is_some_and(|host| host.contains('*'))
    {
        return Err(ProtocolError::InvalidRequest);
    }

    match url.scheme() {
        "https" => {}
        "http" if is_loopback_ip(&url) => {}
        _ => return Err(ProtocolError::InvalidRequest),
    }

    authority_without_port(raw_authority)?;

    if url.query_pairs().any(|(name, _)| {
        matches!(
            name.as_ref(),
            "code" | "state" | "iss" | "error" | "error_description"
        )
    }) {
        return Err(ProtocolError::InvalidRequest);
    }

    Ok(url)
}

fn raw_uri_parts(value: &str) -> Option<(&str, &str, &str)> {
    let (scheme, remainder) = value.split_once("://")?;
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let (authority, suffix) = remainder.split_at_checked(authority_end)?;
    Some((scheme, authority, suffix))
}

fn authority_without_port(authority: &str) -> Result<(&str, Option<u16>), ProtocolError> {
    let invalid = || ProtocolError::InvalidRequest;

    if let Some(bracketed) = authority.strip_prefix('[') {
        let closing_bracket = bracketed.find(']').ok_or_else(invalid)?;
        let host_end = closing_bracket + 2;
        let host = authority.get(..host_end).ok_or_else(invalid)?;
        let suffix = authority.get(host_end..).ok_or_else(invalid)?;
        let Some(raw_port) = suffix.strip_prefix(':') else {
            if suffix.is_empty() {
                return Ok((host, None));
            }
            return Err(invalid());
        };
        return parse_port(raw_port).map(|port| (host, Some(port)));
    }

    match authority.rsplit_once(':') {
        Some((host, raw_port)) => {
            if host.contains(':') || host.is_empty() {
                return Err(invalid());
            }
            parse_port(raw_port).map(|port| (host, Some(port)))
        }
        None if !authority.is_empty() => Ok((authority, None)),
        None => Err(invalid()),
    }
}

fn parse_port(value: &str) -> Result<u16, ProtocolError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProtocolError::InvalidRequest);
    }
    let port = value
        .parse::<u16>()
        .map_err(|_| ProtocolError::InvalidRequest)?;
    if port == 0 {
        return Err(ProtocolError::InvalidRequest);
    }
    Ok(port)
}

fn is_loopback_ip(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(_)) | None => false,
    }
}

fn is_pkce_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn is_bearer_body_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
}

#[cfg(test)]
#[path = "../../unit-tests/oauth/protocol.rs"]
mod tests;
