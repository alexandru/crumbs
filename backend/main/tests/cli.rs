//! Process-level checks for the `crumbs` command-line interface.

use std::io;
use std::process::Output;

mod common;

struct EnvironmentCase<'a> {
    environment: &'a [(&'a str, &'a str)],
    arguments: &'a [&'a str],
    diagnostic: &'a str,
}

#[test]
fn top_level_help_lists_server_command() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["--help"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("server"),
        "unexpected help output: {stdout}"
    );
    Ok(())
}

#[test]
fn server_help_documents_environment_variables_and_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["server", "start", "--help"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "CRUMBS_DATABASE_PATH",
        "CRUMBS_HTTP_BIND",
        "CRUMBS_HTTP_PORT",
        "CRUMBS_OAUTH_ISSUER",
        "--database-path <PATH>",
        "127.0.0.1",
        "8080",
    ] {
        assert!(
            stdout.contains(expected),
            "help omitted {expected}: {stdout}"
        );
    }
    assert!(
        !stdout.contains("data/crumbs.db"),
        "database path had a help default: {stdout}"
    );
    Ok(())
}

#[test]
fn db_import_help_documents_environment_variables_and_has_no_http_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["db", "import-usda", "--help"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "CRUMBS_IMPORT_FILE",
        "CRUMBS_DATABASE_PATH",
        "CRUMBS_VERBOSE",
        "--database-path <PATH>",
        "false",
    ] {
        assert!(
            stdout.contains(expected),
            "help omitted {expected}: {stdout}"
        );
    }
    assert!(
        !stdout.contains("data/crumbs.db"),
        "database path had a help default: {stdout}"
    );
    for unexpected in [
        "CRUMBS_HTTP_BIND",
        "CRUMBS_HTTP_PORT",
        "CRUMBS_OAUTH_ISSUER",
    ] {
        assert!(
            !stdout.contains(unexpected),
            "database help included HTTP setting {unexpected}: {stdout}"
        );
    }
    Ok(())
}

#[test]
fn db_create_user_help_documents_username_environment_variable()
-> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["db", "create-user", "--help"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CRUMBS_USERNAME"),
        "unexpected help output: {stdout}"
    );
    assert!(
        !stdout.contains("CRUMBS_HTTP_BIND"),
        "database help included HTTP settings: {stdout}"
    );
    Ok(())
}

#[test]
fn db_help_lists_import_usda_command() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["db", "--help"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("import-usda"),
        "unexpected help output: {stdout}"
    );
    Ok(())
}

#[test]
fn version_reports_package_version() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["--version"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "unexpected version output: {stdout}"
    );
    Ok(())
}

#[test]
fn missing_subcommand_exits_nonzero() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&[])?;

    assert!(
        !output.status.success(),
        "expected a nonzero exit status, got {}",
        output.status
    );
    Ok(())
}

#[test]
fn invalid_subcommand_exits_nonzero() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["worker"])?;

    assert!(
        !output.status.success(),
        "expected a nonzero exit status, got {}",
        output.status
    );
    Ok(())
}

#[test]
fn server_start_requires_oauth_issuer() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["server", "start", "--database-path", "/tmp/crumbs.db"])?;

    assert_failure_contains(&output, "--oauth-issuer");
    Ok(())
}

#[test]
fn server_start_requires_database_path() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&[
        "server",
        "start",
        "--oauth-issuer",
        "https://crumbs.example.com",
    ])?;

    assert_failure_contains(&output, "--database-path");
    Ok(())
}

#[test]
fn server_start_rejects_removed_config_option() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&[
        "server",
        "start",
        "--database-path",
        "/tmp/crumbs.db",
        "--oauth-issuer",
        "https://crumbs.example.com",
        "--config",
        "/tmp/application.toml",
    ])?;

    assert_failure_contains(&output, "--config");
    Ok(())
}

#[test]
fn server_start_rejects_invalid_http_values_from_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    let cases: [(&[&str], &str); 3] = [
        (
            &[
                "server",
                "start",
                "--database-path",
                "/tmp/crumbs.db",
                "--oauth-issuer",
                "not-a-url",
            ],
            "OAuth issuer must be an HTTPS origin",
        ),
        (
            &[
                "server",
                "start",
                "--database-path",
                "/tmp/crumbs.db",
                "--oauth-issuer",
                "https://crumbs.example.com",
                "--http-port",
                "not-a-port",
            ],
            "invalid digit found in string",
        ),
        (
            &[
                "server",
                "start",
                "--database-path",
                "/tmp/crumbs.db",
                "--oauth-issuer",
                "https://crumbs.example.com",
                "--http-bind",
                "not-an-ip-address",
            ],
            "invalid IP address syntax",
        ),
    ];

    for (arguments, diagnostic) in cases {
        let output = run(arguments)?;
        assert_failure_contains(&output, diagnostic);
    }
    Ok(())
}

#[test]
fn server_start_rejects_invalid_http_values_from_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let valid_issuer = "https://crumbs.example.com";
    let cases = [
        EnvironmentCase {
            environment: &[("CRUMBS_OAUTH_ISSUER", "not-a-url")],
            arguments: &["server", "start", "--database-path", "/tmp/crumbs.db"],
            diagnostic: "OAuth issuer must be an HTTPS origin",
        },
        EnvironmentCase {
            environment: &[("CRUMBS_HTTP_PORT", "not-a-port")],
            arguments: &[
                "server",
                "start",
                "--database-path",
                "/tmp/crumbs.db",
                "--oauth-issuer",
                valid_issuer,
            ],
            diagnostic: "invalid digit found in string",
        },
        EnvironmentCase {
            environment: &[("CRUMBS_HTTP_BIND", "not-an-ip-address")],
            arguments: &[
                "server",
                "start",
                "--database-path",
                "/tmp/crumbs.db",
                "--oauth-issuer",
                valid_issuer,
            ],
            diagnostic: "invalid IP address syntax",
        },
    ];

    for case in cases {
        let output = run_with_env(case.arguments, case.environment)?;
        assert_failure_contains(&output, case.diagnostic);
    }
    Ok(())
}

#[test]
fn db_import_usda_requires_file_when_environment_is_unset() -> Result<(), Box<dyn std::error::Error>>
{
    let output = run(&["db", "import-usda", "--database-path", "/tmp/crumbs.db"])?;

    assert_failure_contains(&output, "<FILE>");
    Ok(())
}

#[test]
fn db_import_usda_requires_database_path() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["db", "import-usda", "/tmp/foundation.json"])?;

    assert_failure_contains(&output, "--database-path");
    Ok(())
}

#[test]
fn db_create_user_requires_database_path() -> Result<(), Box<dyn std::error::Error>> {
    let output = run(&["db", "create-user", "--username", "alice"])?;

    assert_failure_contains(&output, "--database-path");
    Ok(())
}

#[test]
fn create_user_validates_username_from_environment_before_prompting()
-> Result<(), Box<dyn std::error::Error>> {
    let output = run_with_env(
        &["db", "create-user", "--database-path", "/tmp/crumbs.db"],
        &[("CRUMBS_USERNAME", "invalid/name")],
    )?;

    assert_failure_contains(&output, "invalid username");
    Ok(())
}

#[test]
fn create_user_cli_username_overrides_environment() -> Result<(), Box<dyn std::error::Error>> {
    let output = run_with_env(
        &[
            "db",
            "create-user",
            "--database-path",
            "/tmp/crumbs.db",
            "--username",
            "invalid/name",
        ],
        &[("CRUMBS_USERNAME", "alice")],
    )?;

    assert_failure_contains(&output, "invalid username");
    Ok(())
}

fn run(args: &[&str]) -> io::Result<Output> {
    let mut command = common::command();
    command.args(args).output()
}

fn run_with_env(args: &[&str], environment: &[(&str, &str)]) -> io::Result<Output> {
    let mut command = common::command();
    command
        .args(args)
        .envs(environment.iter().copied())
        .output()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure_contains(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains(expected),
        "expected a failure containing {expected:?}, got status {} and stderr: {stderr}",
        output.status
    );
}
