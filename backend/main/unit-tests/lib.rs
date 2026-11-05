use std::ffi::OsString;
use std::path::Path;

use super::{Cli, Command, DatabaseArgs, DbArgs, DbCommand, HttpArgs, ServerArgs, ServerCommand};
use clap::{CommandFactory, FromArgMatches};

#[test]
fn parses_server_start_command() -> Result<(), clap::Error> {
    let cli = parse_without_environment([
        "crumbs",
        "server",
        "start",
        "--database-path",
        "/tmp/crumbs.db",
        "--http-bind",
        "0.0.0.0",
        "--http-port",
        "49152",
        "--oauth-issuer",
        "https://crumbs.example.com",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Server(ServerArgs {
            command: ServerCommand::Start {
                database: DatabaseArgs { path },
                http: HttpArgs { bind, port: 49152, issuer }
            }
        }) if path.as_path() == Path::new("/tmp/crumbs.db")
            && bind.to_string() == "0.0.0.0"
            && issuer.as_str() == "https://crumbs.example.com"
    ));
    Ok(())
}

#[test]
fn rejects_server_start_without_issuer() {
    assert!(
        parse_without_environment([
            "crumbs",
            "server",
            "start",
            "--database-path",
            "/tmp/crumbs.db",
            "--http-bind",
            "127.0.0.1",
            "--http-port",
            "8080",
        ])
        .is_err()
    );
}

#[test]
fn rejects_server_start_without_database_path() {
    assert!(
        parse_without_environment([
            "crumbs",
            "server",
            "start",
            "--oauth-issuer",
            "https://crumbs.example.com",
        ])
        .is_err()
    );
}

#[test]
fn parses_db_import_usda_command_with_database_path() -> Result<(), clap::Error> {
    let cli = parse_without_environment([
        "crumbs",
        "db",
        "import-usda",
        "/tmp/foundation.json",
        "--database-path",
        "/tmp/crumbs.db",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Db(DbArgs {
            command: DbCommand::ImportUsda {
                file,
                database: DatabaseArgs { path },
                verbose: false
            }
        }) if file.as_path() == Path::new("/tmp/foundation.json")
            && path.as_path() == Path::new("/tmp/crumbs.db")
    ));
    Ok(())
}

#[test]
fn parses_db_import_usda_with_database_and_verbose_options() -> Result<(), clap::Error> {
    let cli = parse_without_environment([
        "crumbs",
        "db",
        "import-usda",
        "/tmp/foundation.json",
        "--database-path",
        "/tmp/crumbs.db",
        "--verbose",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Db(DbArgs {
            command: DbCommand::ImportUsda {
                file,
                database: DatabaseArgs { path },
                verbose: true
            }
        }) if file.as_path() == Path::new("/tmp/foundation.json")
            && path.as_path() == Path::new("/tmp/crumbs.db")
    ));
    Ok(())
}

#[test]
fn parses_db_import_usda_verbose_false_value() -> Result<(), clap::Error> {
    let cli = parse_without_environment([
        "crumbs",
        "db",
        "import-usda",
        "/tmp/foundation.json",
        "--database-path",
        "/tmp/crumbs.db",
        "--verbose=false",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Db(DbArgs {
            command: DbCommand::ImportUsda { verbose: false, .. }
        })
    ));
    Ok(())
}

#[test]
fn rejects_db_import_usda_without_database_path() {
    assert!(
        parse_without_environment(["crumbs", "db", "import-usda", "/tmp/foundation.json",])
            .is_err()
    );
}

#[test]
fn parses_db_create_user_with_username_and_database_path() -> Result<(), clap::Error> {
    let cli = parse_without_environment([
        "crumbs",
        "db",
        "create-user",
        "--username",
        "alice",
        "--database-path",
        "/tmp/crumbs.db",
    ])?;

    assert!(matches!(
        cli.command,
        Command::Db(DbArgs {
            command: DbCommand::CreateUser {
                username,
                database: DatabaseArgs { path }
            }
        }) if username == "alice" && path.as_path() == Path::new("/tmp/crumbs.db")
    ));
    Ok(())
}

#[test]
fn rejects_db_create_user_without_username() {
    assert!(
        parse_without_environment([
            "crumbs",
            "db",
            "create-user",
            "--database-path",
            "/tmp/crumbs.db",
        ])
        .is_err()
    );
}

#[test]
fn rejects_db_create_user_without_database_path() {
    assert!(
        parse_without_environment(["crumbs", "db", "create-user", "--username", "alice",]).is_err()
    );
}

#[test]
fn rejects_db_import_usda_without_file() {
    assert!(
        parse_without_environment([
            "crumbs",
            "db",
            "import-usda",
            "--database-path",
            "/tmp/crumbs.db",
        ])
        .is_err()
    );
}

#[test]
fn rejects_removed_config_option() {
    assert!(
        parse_without_environment([
            "crumbs",
            "db",
            "import-usda",
            "/tmp/foundation.json",
            "--database-path",
            "/tmp/crumbs.db",
            "--config",
            "/tmp/application.toml",
        ])
        .is_err()
    );
}

#[test]
fn rejects_missing_subcommand() {
    assert!(parse_without_environment(["crumbs"]).is_err());
}

#[test]
fn rejects_unknown_subcommand() {
    assert!(parse_without_environment(["crumbs", "worker"]).is_err());
}

fn parse_without_environment<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let command = without_environment(Cli::command());
    let matches = command.try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

fn without_environment(command: clap::Command) -> clap::Command {
    command
        .mut_args(|arg| arg.env(None::<&'static str>))
        .mut_subcommands(without_environment)
}
