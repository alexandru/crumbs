//! Process-level checks for `crumbs db import-usda`.

mod common;

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

use common::temporary_config;

type DatabaseFilename<'a> = &'a str;

const BIN: &str = env!("CARGO_BIN_EXE_crumbs");
const FOUNDATION_JSON: &str = include_str!("../../import/test-fixtures/foundation.json");
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
        "Imported 2 foods (0 sanitized); rejected 0 foods\n"
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
fn sqlite_special_names_become_literal_files() -> Result<(), Box<dyn Error>> {
    for database_name in [":memory:", "file::memory:?cache=shared"] {
        assert_literal_database_path(database_name)?;
    }
    Ok(())
}

#[test]
fn malformed_input_fails_with_committed_progress() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("malformed.json");
    fs::write(
        &fixture,
        r#"{"FoundationFoods":[{"fdcId":1,"dataType":"Foundation","description":"committed before syntax error"},{"fdcId":2,}]}"#,
    )?;
    let config = temporary_config(CONFIG)?;

    let output = import(&fixture, config.path())?;

    assert!(
        !output.status.success(),
        "malformed input unexpectedly succeeded"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Imported 1 foods (0 sanitized); rejected 0 foods\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("committed 1 foods"),
        "failure did not report committed progress: {stderr}"
    );
    assert!(
        stderr.contains(&fixture.display().to_string()),
        "failure did not report the input path: {stderr}"
    );
    Ok(())
}

fn assert_literal_database_path(database_name: DatabaseFilename<'_>) -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("foundation.json"), FOUNDATION_JSON)?;
    fs::write(
        directory.path().join("application.toml"),
        format!("[http]\nport = 0\n\n[database]\npath = \"{database_name}\"\n"),
    )?;

    let first = run_import_in(directory.path())?;
    assert_success(&first);
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        "Imported 2 foods (0 sanitized); rejected 0 foods\n"
    );

    let database_path = directory.path().join(database_name);
    assert!(
        database_path.is_file(),
        "literal database file was not created: {}",
        database_path.display()
    );
    let header = fs::read(&database_path)?;
    assert!(
        header.starts_with(b"SQLite format 3\0"),
        "literal database file has no SQLite header: {}",
        database_path.display()
    );

    let second = run_import_in(directory.path())?;
    assert_success(&second);
    assert_eq!(
        String::from_utf8_lossy(&second.stdout),
        "Imported 2 foods (0 sanitized); rejected 0 foods\n"
    );
    assert!(database_path.is_file());
    Ok(())
}

#[test]
fn reports_rejected_and_sanitized_food_counts_with_optional_details() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("mixed.json");
    fs::write(
        &fixture,
        r#"{"FoundationFoods":[
          {"fdcId":11,"dataType":"Foundation","description":"valid one","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]},
          {"fdcId":12,"dataType":"Foundation","description":"negative nutrient","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-2}]},
          {"fdcId":13,"dataType":"Foundation","description":"invalid nutrient","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":"bad"}]},
          {"fdcId":14,"dataType":"Foundation","description":"valid after rejected","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":3}]}
        ]}"#,
    )?;

    let default_config = temporary_config(CONFIG)?;
    let default_output = import(&fixture, default_config.path())?;
    assert!(!default_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&default_output.stdout),
        "Imported 3 foods (1 sanitized); rejected 1 foods\n"
    );
    assert!(String::from_utf8_lossy(&default_output.stderr).contains("rejected 1 USDA foods"));
    assert!(
        !String::from_utf8_lossy(&default_output.stderr).contains("food source="),
        "default output included food-level diagnostics"
    );

    let verbose_config = temporary_config(CONFIG)?;
    let verbose_output = import_verbose(&fixture, verbose_config.path())?;
    assert!(!verbose_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&verbose_output.stdout),
        "Imported 3 foods (1 sanitized); rejected 1 foods\n"
    );
    let stderr = String::from_utf8_lossy(&verbose_output.stderr);
    assert!(
        stderr.contains("sanitized food source=usda source_id=12"),
        "missing sanitized food diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("field=protein"),
        "missing affected field: {stderr}"
    );
    assert!(
        stderr.contains("negative"),
        "missing sanitization reason: {stderr}"
    );
    assert!(
        stderr.contains("rejected food source=usda source_id=13"),
        "missing rejected food diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("invalid"),
        "missing rejection reason: {stderr}"
    );
    assert!(stderr.contains("rejected 1 USDA foods"));
    Ok(())
}

#[test]
fn sanitized_foods_are_successful_without_verbose_output() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("sanitized.json");
    fs::write(
        &fixture,
        r#"{"FoundationFoods":[{"fdcId":21,"dataType":"Foundation","description":"negative nutrient","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-2}]}]}"#,
    )?;
    let config = temporary_config(CONFIG)?;

    let output = import(&fixture, config.path())?;

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Imported 1 foods (1 sanitized); rejected 0 foods\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    Ok(())
}

fn run_import_in(directory: &Path) -> io::Result<Output> {
    Command::new(BIN)
        .current_dir(directory)
        .args([
            "db",
            "import-usda",
            "foundation.json",
            "--config",
            "application.toml",
        ])
        .output()
}

fn import(fixture: &std::path::Path, config: &std::path::Path) -> io::Result<Output> {
    Command::new(BIN)
        .args(["db", "import-usda"])
        .arg(fixture)
        .arg("--config")
        .arg(config)
        .output()
}

fn import_verbose(fixture: &std::path::Path, config: &std::path::Path) -> io::Result<Output> {
    Command::new(BIN)
        .args(["db", "import-usda"])
        .arg(fixture)
        .arg("--config")
        .arg(config)
        .arg("--verbose")
        .output()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
