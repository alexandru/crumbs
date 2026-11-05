//! Process-level checks for `crumbs db import-usda`.

mod common;

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Output;

use common::command;

type DatabaseFilename<'a> = &'a str;

const FOUNDATION_JSON: &str = include_str!("../../import/test-fixtures/foundation.json");

#[test]
fn imports_foundation_fixture_without_http_configuration() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("foundation.json");
    let database_path = directory.path().join("crumbs.db");
    fs::write(&fixture, FOUNDATION_JSON)?;

    let output = import_with(
        &fixture,
        &database_path,
        &[],
        &[
            ("CRUMBS_IMPORT_FILE", "/a/nonexistent/foundation.json"),
            ("CRUMBS_HTTP_BIND", "not-an-ip-address"),
            ("CRUMBS_HTTP_PORT", "not-a-port"),
            ("CRUMBS_OAUTH_ISSUER", "not-a-url"),
            ("CRUMBS_DATABASE_PATH", "/a/nonexistent/parent/crumbs.db"),
        ],
    )?;

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Imported 2 foods (0 sanitized); rejected 0 foods\n"
    );
    assert!(
        database_path.is_file(),
        "import did not create {}",
        database_path.display()
    );
    Ok(())
}

#[test]
fn import_file_can_come_from_environment() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("foundation.json");
    let database_path = directory.path().join("crumbs.db");
    fs::write(&fixture, FOUNDATION_JSON)?;

    let output = run_import(
        &[],
        &[
            (
                "CRUMBS_IMPORT_FILE",
                fixture
                    .to_str()
                    .ok_or_else(|| io::Error::other("fixture path is not valid UTF-8"))?,
            ),
            (
                "CRUMBS_DATABASE_PATH",
                database_path
                    .to_str()
                    .ok_or_else(|| io::Error::other("database path is not valid UTF-8"))?,
            ),
        ],
        None,
    )?;

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Imported 2 foods (0 sanitized); rejected 0 foods\n"
    );
    assert!(database_path.is_file());
    Ok(())
}

#[test]
fn relative_database_path_uses_the_working_directory() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("foundation.json"), FOUNDATION_JSON)?;

    let output = run_import(
        &[
            OsString::from("foundation.json"),
            OsString::from("--database-path"),
            OsString::from("crumbs.db"),
        ],
        &[],
        Some(directory.path()),
    )?;

    assert_success(&output);
    assert!(directory.path().join("crumbs.db").is_file());
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
    let database_path = directory.path().join("crumbs.db");
    fs::write(
        &fixture,
        r#"{"FoundationFoods":[{"fdcId":1,"dataType":"Foundation","description":"committed before syntax error"},{"fdcId":2,}]}"#,
    )?;

    let output = import(&fixture, &database_path)?;

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

    let arguments = [
        OsString::from("foundation.json"),
        OsString::from("--database-path"),
        OsString::from(database_name),
    ];
    let first = run_import(&arguments, &[], Some(directory.path()))?;
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

    let second = run_import(&arguments, &[], Some(directory.path()))?;
    assert_success(&second);
    assert_eq!(
        String::from_utf8_lossy(&second.stdout),
        "Imported 2 foods (0 sanitized); rejected 0 foods\n"
    );
    assert!(database_path.is_file());
    Ok(())
}

#[test]
fn reports_food_level_sanitization_and_identity_rejection_details() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("mixed.json");
    let database_path = directory.path().join("crumbs.db");
    fs::write(
        &fixture,
        r#"{"FoundationFoods":[
          {"fdcId":11,"dataType":"Foundation","description":"valid one","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]},
          {"fdcId":12,"dataType":"Foundation","description":"negative nutrient","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-2}]},
          {"fdcId":13,"dataType":"Foundation","description":"invalid nutrient","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":"bad"}]},
          {"fdcId":14,"dataType":"Foundation","description":"valid after sanitized","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":3}]},
          {"fdcId":15,"dataType":"Foundation","foodNutrients":[]}
        ]}"#,
    )?;

    let default_output = import(&fixture, &database_path)?;
    assert!(!default_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&default_output.stdout),
        "Imported 4 foods (2 sanitized); rejected 1 foods\n"
    );
    assert!(String::from_utf8_lossy(&default_output.stderr).contains("rejected 1 USDA foods"));
    assert!(
        !String::from_utf8_lossy(&default_output.stderr).contains("food source="),
        "default output included food-level diagnostics"
    );

    let quiet_override = import_with(
        &fixture,
        &directory.path().join("quiet.db"),
        &["--verbose=false"],
        &[("CRUMBS_VERBOSE", "true")],
    )?;
    assert!(!quiet_override.status.success());
    assert!(
        !String::from_utf8_lossy(&quiet_override.stderr).contains("food source="),
        "CLI false did not override CRUMBS_VERBOSE=true"
    );

    let verbose_output = import_with(
        &fixture,
        &directory.path().join("verbose.db"),
        &[],
        &[("CRUMBS_VERBOSE", "true")],
    )?;
    assert!(!verbose_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&verbose_output.stdout),
        "Imported 4 foods (2 sanitized); rejected 1 foods\n"
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
        stderr.contains("sanitized food source=usda source_id=13"),
        "missing sanitized optional nutrient diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("rejected food source=usda source_id=15"),
        "missing rejected food diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("nutrient amount"),
        "missing sanitization reason: {stderr}"
    );
    assert!(stderr.contains("rejected 1 USDA foods"));
    Ok(())
}

#[test]
fn sanitized_foods_are_successful_without_verbose_output() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("sanitized.json");
    let database_path = directory.path().join("crumbs.db");
    fs::write(
        &fixture,
        r#"{"FoundationFoods":[{"fdcId":21,"dataType":"Foundation","description":"negative nutrient","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-2}]}]}"#,
    )?;

    let output = import(&fixture, &database_path)?;

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Imported 1 foods (1 sanitized); rejected 0 foods\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    Ok(())
}

fn import(fixture: &Path, database_path: &Path) -> io::Result<Output> {
    import_with(fixture, database_path, &[], &[])
}

fn import_with(
    fixture: &Path,
    database_path: &Path,
    options: &[&str],
    environment: &[(&str, &str)],
) -> io::Result<Output> {
    let arguments = [
        fixture.as_os_str().to_owned(),
        OsString::from("--database-path"),
        database_path.as_os_str().to_owned(),
    ];
    let all_arguments = arguments
        .into_iter()
        .chain(options.iter().map(|option| OsString::from(*option)))
        .collect::<Vec<_>>();
    run_import(&all_arguments, environment, None)
}

fn run_import(
    arguments: &[OsString],
    environment: &[(&str, &str)],
    directory: Option<&Path>,
) -> io::Result<Output> {
    let mut command = command();
    command.args(["db", "import-usda"]).args(arguments);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    command.envs(environment.iter().copied());
    command.output()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
