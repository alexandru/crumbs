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
mod tests {
    use super::Database;

    #[test]
    fn default_database_is_send_sync_and_debuggable() {
        fn require_traits<T: Send + Sync + Default + std::fmt::Debug>() {}
        require_traits::<Database>();

        let database = Database;
        assert_eq!(format!("{database:?}"), "Database");
    }
}
