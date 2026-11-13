//! Persistent storage for Crumbs.
//!
//! [`Database`] prepares storage before exposing a usable handle and is passed
//! to the server at startup. Dropping the handle initiates cleanup;
//! [`Database::close`] waits for it.

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
