//! the cross-dialect parity body, run against sqlite.
//!
//! The MariaDB and PostgreSQL suites skip without a live URL. In a normal workspace this
//! is the only thing that executes `dialect_parity` at all. without it the shared body would still
//! compile but never run, and would rot into something that fails the moment someone brings the
//! engines up — which is precisely when it is least useful.

use super::*;
use crate::dialect_parity::assert_dialect_parity;

#[tokio::test]
async fn sqlite_lifecycle() {
    let path = std::env::temp_dir().join(format!(
        "runinator-parity-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    assert_dialect_parity(&db).await;

    let _ = fs::remove_file(path);
}
