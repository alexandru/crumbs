//! Process-level HTTP behavior for `crumbs server start`.

mod common;

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::Path;

use serde_json::{Value, json};

use common::{CommandOutput, HttpStatusCode, ServerProcess, get, run_server_to_exit, server_args};

#[test]
fn root_is_public_and_food_apis_require_bearer_tokens() -> Result<(), Box<dyn Error>> {
    let server = ServerProcess::spawn()?;
    assert_eq!(server.address().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));

    let response = get(&server, "/")?;

    assert_eq!(response.status(), HttpStatusCode::OK);
    assert!(response.headers().get("content-type").is_some_and(|value| {
        value
            .to_str()
            .is_ok_and(|value| value.starts_with("text/html"))
    }));
    for text in [
        "Hello, welcome to Crumbs.",
        "A little clarity about what you eat.",
        "Your food library is ready.",
    ] {
        assert!(
            response.body().contains(text),
            "missing welcome text {text:?}"
        );
    }

    for path in ["/foods/search?query=chicken", "/mcp"] {
        let protected = get(&server, path)?;
        assert_eq!(
            protected.status(),
            HttpStatusCode::UNAUTHORIZED,
            "{path} should require a bearer token"
        );
        assert!(protected.headers().contains_key("www-authenticate"));
    }
    Ok(())
}

#[test]
fn server_configuration_can_come_from_environment() -> Result<(), Box<dyn Error>> {
    let server = ServerProcess::spawn_env_only()?;

    assert!(server.directory().join("crumbs.db").is_file());
    let response = get(&server, "/.well-known/oauth-authorization-server")?;
    assert_eq!(response.status(), HttpStatusCode::OK);
    let body: Value = serde_json::from_str(response.body())?;
    assert_eq!(body["issuer"], "http://127.0.0.1:9191");
    Ok(())
}

#[test]
fn command_line_values_override_invalid_configuration_environment() -> Result<(), Box<dyn Error>> {
    let arguments = server_args(
        Path::new("crumbs-cli.db"),
        "127.0.0.1",
        "0",
        "http://127.0.0.1:8181",
    );
    let server = ServerProcess::spawn_with(
        &arguments,
        &[
            ("CRUMBS_DATABASE_PATH", "/a/nonexistent/parent/crumbs.db"),
            ("CRUMBS_HTTP_BIND", "not-an-ip-address"),
            ("CRUMBS_HTTP_PORT", "not-a-port"),
            ("CRUMBS_OAUTH_ISSUER", "not-a-url"),
        ],
    )?;

    assert!(server.directory().join("crumbs-cli.db").is_file());
    let response = get(&server, "/.well-known/oauth-authorization-server")?;
    assert_eq!(response.status(), HttpStatusCode::OK);
    let body: Value = serde_json::from_str(response.body())?;
    assert_eq!(body["issuer"], "http://127.0.0.1:8181");
    Ok(())
}

#[test]
fn https_issuer_allows_public_listener_for_a_tls_proxy() -> Result<(), Box<dyn Error>> {
    let arguments = server_args(
        Path::new("crumbs.db"),
        "0.0.0.0",
        "0",
        "https://crumbs.example.com",
    );
    let server = ServerProcess::spawn_with(&arguments, &[])?;

    assert_eq!(server.address().ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    Ok(())
}

#[test]
fn oauth_discovery_is_available_without_authentication() -> Result<(), Box<dyn Error>> {
    let server = ServerProcess::spawn()?;

    let response = get(&server, "/.well-known/oauth-authorization-server")?;

    assert_eq!(response.status(), HttpStatusCode::OK);
    let body: Value = serde_json::from_str(response.body())?;
    assert_eq!(body["issuer"], "http://127.0.0.1");
    assert_eq!(body["code_challenge_methods_supported"], json!(["S256"]));
    Ok(())
}

#[test]
fn unknown_route_returns_not_found() -> Result<(), Box<dyn Error>> {
    let server = ServerProcess::spawn()?;

    let response = get(&server, "/missing")?;

    assert_eq!(response.status(), HttpStatusCode::NOT_FOUND);
    Ok(())
}

#[test]
fn startup_creates_the_configured_database_file() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("nested").join("crumbs.db");
    let arguments = server_args(&database_path, "127.0.0.1", "0", "http://127.0.0.1");
    let server = ServerProcess::spawn_with(&arguments, &[])?;

    assert!(
        database_path.is_file(),
        "startup did not create {}",
        database_path.display()
    );
    let response = get(&server, "/")?;
    assert_eq!(response.status(), HttpStatusCode::OK);
    Ok(())
}

#[test]
fn occupied_port_fails_before_serving() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
    let port = listener.local_addr()?.port();
    let arguments = server_args(
        Path::new("crumbs.db"),
        "0.0.0.0",
        &port.to_string(),
        "https://crumbs.example.com",
    );

    assert_failed_start(&run_server_to_exit(&arguments, &[])?);
    Ok(())
}

#[test]
fn invalid_server_environment_value_fails_before_serving() -> Result<(), Box<dyn Error>> {
    let arguments = [
        OsString::from("--database-path"),
        OsString::from("crumbs.db"),
        OsString::from("--http-bind"),
        OsString::from("127.0.0.1"),
    ];
    let output = run_server_to_exit(
        &arguments,
        &[
            ("CRUMBS_HTTP_PORT", "not-a-port"),
            ("CRUMBS_OAUTH_ISSUER", "http://127.0.0.1"),
        ],
    )?;

    assert_failed_start(&output);
    assert!(
        output.stderr.contains("--http-port"),
        "invalid environment value was not attributed to the port option: {}",
        output.stderr
    );
    Ok(())
}

#[test]
fn loopback_http_issuer_cannot_bind_a_public_interface() -> Result<(), Box<dyn Error>> {
    let arguments = server_args(
        Path::new("crumbs.db"),
        "0.0.0.0",
        "0",
        "http://127.0.0.1:8080",
    );

    let output = run_server_to_exit(&arguments, &[])?;

    assert_failed_start(&output);
    assert!(
        output
            .stderr
            .contains("HTTP OAuth issuer requires a loopback bind address"),
        "unexpected startup error: {}",
        output.stderr
    );
    Ok(())
}

#[test]
fn database_initialization_failure_fails_before_serving() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let blocker = directory.path().join("blocker");
    fs::write(&blocker, b"not a directory")?;
    let database_path = blocker.join("nested").join("crumbs.db");
    let arguments = server_args(&database_path, "127.0.0.1", "0", "http://127.0.0.1");

    let output = run_server_to_exit(&arguments, &[])?;
    assert_failed_start(&output);
    assert!(
        output.stderr.contains(&blocker.display().to_string()),
        "database failure did not name the failing path: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Not a directory"),
        "database failure did not report the filesystem cause: {}",
        output.stderr
    );
    Ok(())
}

fn assert_failed_start(output: &CommandOutput) {
    assert!(
        !output.status.success(),
        "expected server startup to fail, got {}",
        output.status
    );
    assert!(
        output.stdout.is_empty(),
        "failed startup wrote a readiness message: {}",
        output.stdout
    );
    assert!(
        !output.stderr.is_empty(),
        "failed startup did not report an error"
    );
}
