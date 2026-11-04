use std::path::Path;

use super::{ServerArgs, ServerCommand};
use clap::Parser;

#[derive(Debug, Parser)]
struct TestCli {
    #[command(subcommand)]
    command: TestCommand,
}

#[derive(Debug, clap::Subcommand)]
enum TestCommand {
    Server(ServerArgs),
}

#[test]
fn parses_nested_server_start_command() -> Result<(), clap::Error> {
    let cli = TestCli::try_parse_from([
        "crumbs",
        "server",
        "start",
        "--config",
        "/tmp/application.toml",
    ])?;

    assert!(matches!(
        cli.command,
        TestCommand::Server(ServerArgs {
            command: ServerCommand::Start { config }
        }) if config.as_path() == Path::new("/tmp/application.toml")
    ));
    Ok(())
}

#[test]
fn rejects_server_start_without_config() {
    assert!(TestCli::try_parse_from(["crumbs", "server", "start"]).is_err());
}

#[test]
fn rejects_unknown_server_operation() {
    assert!(TestCli::try_parse_from(["crumbs", "server", "stop"]).is_err());
}
