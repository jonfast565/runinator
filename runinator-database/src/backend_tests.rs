use super::backend::{is_transient_delete_database_error, is_transient_delete_error};

#[test]
fn non_database_errors_are_not_transient_delete_errors() {
    assert!(!is_transient_delete_error(&sqlx::Error::RowNotFound));
    assert!(!is_transient_delete_error(&sqlx::Error::PoolTimedOut));
}

#[test]
fn backend_deadlock_and_lock_codes_are_transient_delete_errors() {
    for code in [
        "40001", "40P01", "55P03", "1205", "1213", "5", "6", "261", "262", "517", "518", "773",
    ] {
        assert!(
            is_transient_delete_database_error(Some(code), ""),
            "expected {code} to be retryable"
        );
    }
}

#[test]
fn permanent_database_errors_are_not_transient_delete_errors() {
    assert!(!is_transient_delete_database_error(
        Some("23503"),
        "foreign key violation"
    ));
    assert!(!is_transient_delete_database_error(
        Some("1062"),
        "duplicate entry"
    ));
}

#[test]
fn sqlite_lock_messages_are_transient_without_a_code() {
    assert!(is_transient_delete_database_error(
        None,
        "database is locked"
    ));
    assert!(is_transient_delete_database_error(None, "Database is busy"));
    assert!(is_transient_delete_database_error(
        None,
        "database table is locked"
    ));
}
