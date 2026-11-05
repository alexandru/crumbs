//! Command-line arguments and environment settings for each operation.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use crumbs_database::{DatabaseConfig, DatabasePath};
use crumbs_server::{HttpBindAddress, HttpConfig, HttpPort, IssuerUrl, OAuthConfig};

/// Arguments accepted by the `server` subcommand.
#[derive(Debug, Args)]
pub struct ServerArgs {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: ServerCommand,
}

/// Operations exposed by the `server` subcommand.
#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Start the server and wait for a shutdown signal.
    Start {
        #[command(flatten)]
        database: DatabaseArgs,
        #[command(flatten)]
        http: HttpArgs,
    },
}

/// Database settings shared by commands that open the database.
#[derive(Debug, Args)]
pub struct DatabaseArgs {
    /// SQLite database file. Relative paths use the current working directory.
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

/// HTTP settings required only when starting the server.
#[derive(Debug, Args)]
pub struct HttpArgs {
    /// IP address to bind. Use 0.0.0.0 inside a container behind a TLS proxy.
    #[arg(
        long = "http-bind",
        env = "CRUMBS_HTTP_BIND",
        value_name = "IP",
        default_value = "127.0.0.1"
    )]
    pub bind: HttpBindAddress,
    /// TCP port to listen on. Use 0 to choose an available port.
    #[arg(
        long = "http-port",
        env = "CRUMBS_HTTP_PORT",
        value_name = "PORT",
        default_value = "8080"
    )]
    pub port: HttpPort,
    /// Public HTTPS origin, or an HTTP loopback IP origin for development.
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

/// Arguments accepted by the `db` subcommand.
#[derive(Debug, Args)]
pub struct DbArgs {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: DbCommand,
}

/// Operations exposed by the `db` subcommand.
#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Import an uncompressed USDA FoodData Central JSON download.
    ImportUsda {
        /// Path to an uncompressed USDA FoodData Central JSON file (not ZIP).
        #[arg(value_name = "FILE", env = "CRUMBS_IMPORT_FILE")]
        file: PathBuf,
        #[command(flatten)]
        database: DatabaseArgs,
        /// Print details for foods with sanitized or rejected data.
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
    /// Create a user account.
    CreateUser {
        /// Username for the new account.
        #[arg(long, env = "CRUMBS_USERNAME", value_name = "NAME")]
        username: String,
        #[command(flatten)]
        database: DatabaseArgs,
    },
}
