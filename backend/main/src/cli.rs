use std::path::PathBuf;

use clap::{Args, Subcommand};
use crumbs_database::{DatabaseConfig, DatabasePath};
use crumbs_server::{HttpBindAddress, HttpConfig, HttpPort, IssuerUrl, OAuthConfig};

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub command: ServerCommand,
}

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    #[command(about = "Start the server and wait for a shutdown signal")]
    Start {
        #[command(flatten)]
        database: DatabaseArgs,
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Debug, Args)]
pub struct DatabaseArgs {
    #[arg(help = "SQLite database file. Relative paths use the current working directory")]
    #[arg(
        long = "database-path",
        env = "CRUMBS_DATABASE_PATH",
        value_name = "PATH"
    )]
    pub path: DatabasePath,
}

impl From<DatabaseArgs> for DatabaseConfig {
    fn from(args: DatabaseArgs) -> Self {
        Self { path: args.path }
    }
}

#[derive(Debug, Args)]
pub struct HttpArgs {
    #[arg(help = "IP address to bind. Use 0.0.0.0 inside a container behind a TLS proxy")]
    #[arg(
        long = "http-bind",
        env = "CRUMBS_HTTP_BIND",
        value_name = "IP",
        default_value = "127.0.0.1"
    )]
    pub bind: HttpBindAddress,
    #[arg(help = "TCP port to listen on. Use 0 to choose an available port")]
    #[arg(
        long = "http-port",
        env = "CRUMBS_HTTP_PORT",
        value_name = "PORT",
        default_value = "8080"
    )]
    pub port: HttpPort,
    #[arg(help = "Public HTTPS origin, or an HTTP loopback IP origin for development")]
    #[arg(
        long = "oauth-issuer",
        env = "CRUMBS_OAUTH_ISSUER",
        value_name = "URL",
        value_parser = IssuerUrl::new
    )]
    pub issuer: IssuerUrl,
}

impl From<HttpArgs> for HttpConfig {
    fn from(args: HttpArgs) -> Self {
        Self {
            bind: args.bind,
            port: args.port,
            oauth: OAuthConfig {
                issuer: args.issuer,
            },
        }
    }
}

#[derive(Debug, Args)]
pub struct DbArgs {
    #[command(subcommand)]
    pub command: DbCommand,
}

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    #[command(about = "Import an uncompressed USDA FoodData Central JSON download")]
    ImportUsda {
        #[arg(help = "Path to an uncompressed USDA FoodData Central JSON file (not ZIP)")]
        #[arg(value_name = "FILE", env = "CRUMBS_IMPORT_FILE")]
        file: PathBuf,
        #[command(flatten)]
        database: DatabaseArgs,
        #[arg(help = "Print details for foods with sanitized or rejected data")]
        #[arg(
            long,
            env = "CRUMBS_VERBOSE",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "true",
            default_value = "false",
            action = clap::ArgAction::Set
        )]
        verbose: bool,
    },
    #[command(about = "Create a user account")]
    CreateUser {
        #[arg(help = "Username for the new account")]
        #[arg(long, env = "CRUMBS_USERNAME", value_name = "NAME")]
        username: String,
        #[command(flatten)]
        database: DatabaseArgs,
    },
}
