//! Executable entry point for the Crumbs backend.

use crumbs_server::Error;

fn main() -> Result<(), Error> {
    crumbs_main::run()
}
