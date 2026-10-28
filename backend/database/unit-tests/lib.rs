use super::Database;

#[test]
fn default_database_is_send_sync_and_debuggable() {
    fn require_traits<T: Send + Sync + Default + std::fmt::Debug>() {}
    require_traits::<Database>();

    let database = Database;
    assert_eq!(format!("{database:?}"), "Database");
}
