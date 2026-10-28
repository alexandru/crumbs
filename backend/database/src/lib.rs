//! Persistent storage for Crumbs.
//!
//! [`Database`] is the handle that the executable creates and passes to the
//! server at startup.

/// Handle to the application database.
///
/// Construct it with [`Database::default`] and pass it to the server, which owns
/// it for the lifetime of the process.
#[derive(Debug, Default)]
pub struct Database;

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
