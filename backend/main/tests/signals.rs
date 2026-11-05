//! Process-level shutdown checks for `crumbs server start`.
//!
//! Each test performs a real HTTP request first, then verifies that a Unix
//! signal produces a clean exit.

mod common;

use std::error::Error;

use common::{HttpStatusCode, SHUTDOWN_TIMEOUT, ServerProcess};
use nix::sys::signal::Signal;

#[test]
fn terminates_cleanly_on_hangup() -> Result<(), Box<dyn Error>> {
    assert_clean_shutdown(Signal::SIGHUP)
}

#[test]
fn terminates_cleanly_on_terminate() -> Result<(), Box<dyn Error>> {
    assert_clean_shutdown(Signal::SIGTERM)
}

#[test]
fn terminates_cleanly_on_interrupt() -> Result<(), Box<dyn Error>> {
    assert_clean_shutdown(Signal::SIGINT)
}

fn assert_clean_shutdown(signal: Signal) -> Result<(), Box<dyn Error>> {
    let mut server = ServerProcess::spawn()?;

    let response = common::get(&server, "/.well-known/oauth-authorization-server")?;
    assert_eq!(
        response.status(),
        HttpStatusCode::OK,
        "OAuth discovery did not answer before {signal:?}"
    );

    server.signal(signal)?;
    let status = server.wait_for_exit(SHUTDOWN_TIMEOUT)?;

    assert!(
        status.success(),
        "server exited with {status} after {signal:?}"
    );
    Ok(())
}
