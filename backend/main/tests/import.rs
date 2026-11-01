//! Process-level checks for `crumbs db import-usda`.

mod common;

use std::error::Error;
use std::fs;
use std::io;
use std::process::{Command, Output};

use common::temporary_config;

const BIN: &str = env!("CARGO_BIN_EXE_crumbs");
const FOUNDATION_JSON: &str = include_str!("../../database/test-fixtures/foundation.json");
const CONFIG: &str = "[http]\nport = 0\n\n[database]\npath = \"crumbs.db\"\n";

#[test]
fn imports_foundation_fixture_and_reports_count() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("foundation.json");
    fs::write(&fixture, FOUNDATION_JSON)?;
    let config = temporary_config(CONFIG)?;

    let output = import(&fixture, config.path())?;

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Imported 2 foods\n"
    );
    let database_path = config
        .path()
        .parent()
        .ok_or_else(|| io::Error::other("temporary config has no parent"))?
        .join("crumbs.db");
    assert!(
        database_path.is_file(),
        "import did not create {}",
        database_path.display()
    );
    Ok(())
}

#[test]
fn malformed_input_fails_with_committed_progress() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("malformed.json");
    fs::write(&fixture, r#"{"FoundationFoods":[{"fdcId":1,}]}"#)?;
    let config = temporary_config(CONFIG)?;

    let output = import(&fixture, config.path())?;

    assert!(
        !output.status.success(),
        "malformed input unexpectedly succeeded"
    );
    assert!(
        output.stdout.is_empty(),
        "failed import wrote a success message: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("committed 0 foods"),
        "failure did not report committed progress: {stderr}"
    );
    assert!(
        stderr.contains(&fixture.display().to_string()),
        "failure did not report the input path: {stderr}"
    );
    Ok(())
}

fn import(fixture: &std::path::Path, config: &std::path::Path) -> io::Result<Output> {
    Command::new(BIN)
        .args(["db", "import-usda"])
        .arg(fixture)
        .arg("--config")
        .arg(config)
        .output()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
