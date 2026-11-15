mod clock;
mod config;
pub mod foods;
pub mod identity;
pub mod nutrition_model;
pub mod profile_logs;
pub mod profiles;
mod runtime;
pub mod text;

pub use config::{DatabaseConfig, DatabasePath};
pub use runtime::{Database, Error};

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
