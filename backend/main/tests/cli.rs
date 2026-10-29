//! Process-level checks for the `crumbs` command-line interface.

use std::error::Error;
use std::io;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_crumbs");

#[test]
fn top_level_help_lists_server_command() -> Result<(), Box<dyn Error>> {
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
fn server_help_lists_start_command() -> Result<(), Box<dyn Error>> {
    let output = run(&["server", "--help"])?;
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("start"), "unexpected help output: {stdout}");
    Ok(())
}

#[test]
fn db_help_lists_import_usda_command() -> Result<(), Box<dyn Error>> {
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
fn version_reports_package_version() -> Result<(), Box<dyn Error>> {
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
fn missing_subcommand_exits_nonzero() -> Result<(), Box<dyn Error>> {
    let output = run(&[])?;

    assert!(
        !output.status.success(),
        "expected a nonzero exit status, got {}",
        output.status
    );
    Ok(())
}

#[test]
fn invalid_subcommand_exits_nonzero() -> Result<(), Box<dyn Error>> {
    let output = run(&["worker"])?;

    assert!(
        !output.status.success(),
        "expected a nonzero exit status, got {}",
        output.status
    );
    Ok(())
}

#[test]
fn server_start_requires_config() -> Result<(), Box<dyn Error>> {
    let output = run(&["server", "start"])?;

    assert!(
        !output.status.success(),
        "expected a nonzero exit status, got {}",
        output.status
    );
    Ok(())
}

fn run(args: &[&str]) -> io::Result<Output> {
    Command::new(BIN).args(args).output()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
