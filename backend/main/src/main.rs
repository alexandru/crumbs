//! Executable entry point for the Crumbs backend.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match crumbs_main::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut stderr = io::stderr().lock();
            let _ = writeln!(stderr, "{error}");
            ExitCode::FAILURE
        }
    }
}
