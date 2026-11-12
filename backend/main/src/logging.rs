//! Process-owned logging configuration.

use std::error::Error as StdError;

use tracing_subscriber::EnvFilter;

pub(crate) fn init() -> Result<(), Box<dyn StdError + Send + Sync>> {
    let filter = EnvFilter::builder()
        .with_default_directive("crumbs=info".parse()?)
        .from_env_lossy();

    tracing_subscriber::fmt()
        .compact()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
}
