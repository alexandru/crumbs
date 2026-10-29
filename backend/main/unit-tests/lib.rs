use std::path::Path;

use super::{Cli, Command};
use clap::Parser;
use crumbs_database::{DbArgs, DbCommand};
use crumbs_server::{ServerArgs, ServerCommand};

#[test]
fn parses_server_start_command() -> Result<(), clap::Error> {
    let cli = Cli::try_parse_from([
        "crumbs",
        "server",
        "start",
        "--config",
        "/tmp/application.toml",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Server(ServerArgs {
            command: ServerCommand::Start { config }
        }) if config.as_path() == Path::new("/tmp/application.toml")
    ));
    Ok(())
}

#[test]
fn rejects_server_start_without_config() {
    assert!(Cli::try_parse_from(["crumbs", "server", "start"]).is_err());
}

#[test]
fn parses_db_import_usda_command() -> Result<(), clap::Error> {
    let cli = Cli::try_parse_from([
        "crumbs",
        "db",
        "import-usda",
        "/tmp/foundation.json",
        "--config",
        "/tmp/application.toml",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Db(DbArgs {
            command: DbCommand::ImportUsda { file, config }
        }) if file.as_path() == Path::new("/tmp/foundation.json")
            && config.as_path() == Path::new("/tmp/application.toml")
    ));
    Ok(())
}

#[test]
fn rejects_db_import_usda_without_file() {
    assert!(
        Cli::try_parse_from([
            "crumbs",
            "db",
            "import-usda",
            "--config",
            "/tmp/application.toml",
        ])
        .is_err()
    );
}

#[test]
fn rejects_db_import_usda_without_config() {
    assert!(Cli::try_parse_from(["crumbs", "db", "import-usda", "/tmp/foundation.json"]).is_err());
}

#[test]
fn rejects_missing_subcommand() {
    assert!(Cli::try_parse_from(["crumbs"]).is_err());
}

#[test]
fn rejects_unknown_subcommand() {
    assert!(Cli::try_parse_from(["crumbs", "worker"]).is_err());
}
