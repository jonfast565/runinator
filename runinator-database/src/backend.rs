//! the per-backend seam.
//!
//! a backend exposes only what genuinely differs between databases: the concrete sqlx pool and the
//! sql dialect. every `DatabaseImpl` method body is written once as a blanket impl over `SqlBackend`
//! in `crate::operations`, so adding a database means implementing this trait, not re-typing queries.

use std::{future::Future, time::Duration};

use log::warn;
use runinator_models::errors::SendableError;
#[cfg(feature = "mariadb")]
use sqlx::mysql::MySqlQueryResult;
#[cfg(feature = "postgres")]
use sqlx::postgres::PgQueryResult;
#[cfg(feature = "sqlite")]
use sqlx::sqlite::SqliteQueryResult;
use sqlx::{AssertSqlSafe, Database, Pool, SqlSafeStr, SqlStr};

use crate::queries::SqlDialect;

const DELETE_RETRY_LIMIT: usize = 4;
const DELETE_RETRY_BASE_DELAY: Duration = Duration::from_millis(10);

/// sql generated exclusively from the database dialect and repository-owned templates.

/// retry a delete when the database chose it as the victim of a transient lock conflict.
///
/// deadlock detection aborts the statement (and its transaction), so retrying the whole logical
/// delete is the portable recovery path. callers must keep `operation` idempotent and include every
/// statement in one transaction when the delete spans multiple tables.
pub(crate) async fn retry_delete<T, F, Fut>(mut operation: F) -> Result<T, sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, sqlx::Error>>,
{
    for attempt in 0..DELETE_RETRY_LIMIT {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if attempt + 1 < DELETE_RETRY_LIMIT && is_transient_delete_error(&error) => {
                let delay = DELETE_RETRY_BASE_DELAY * (1 << attempt);
                warn!(
                    "transient database lock conflict during delete; retrying in {} ms: {error}",
                    delay.as_millis()
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the delete retry loop always returns on its final attempt")
}

pub(super) fn is_transient_delete_error(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(error) = error else {
        return false;
    };
    let code = error.code();
    is_transient_delete_database_error(code.as_deref(), error.message())
}

pub(super) fn is_transient_delete_database_error(code: Option<&str>, message: &str) -> bool {
    if matches!(
        code,
        // postgres: serialization failure, deadlock detected, lock not available.
        Some("40001" | "40P01" | "55P03")
            // mysql: lock wait timeout, deadlock victim.
            | Some("1205" | "1213")
            // sqlite: busy, locked (including their extended result codes).
            | Some("5" | "6" | "261" | "262" | "517" | "518" | "773")
    ) {
        return true;
    }

    // sqlite's driver does not expose a numeric code for every busy/locked variant.
    let message = message.to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database schema is locked")
        || message.contains("database is busy")
}

/// portable access to a statement's affected-row count.
///
/// each driver exposes `rows_affected` as an inherent method on its own `QueryResult`; this trait
/// lets generic code read it through `Database::QueryResult`.

/// the connection + dialect a generic database operation runs against.

/// a backend wearing the `DatabaseImpl` contract.
///
/// `DatabaseImpl` is defined in `runinator-store`, which is deliberately sqlx-free, so the orphan
/// rule forbids implementing it on a bare type parameter here. wrapping the backend in a type this
/// crate owns restores that: the 200-plus method bodies in `crate::operations` stay written once,
/// generically, rather than once per driver.
///
/// each driver module aliases this (`pub type SqliteDb = SqlStore<SqliteBackend>`) and supplies its
/// own `new`, so callers keep naming `SqliteDb`/`PostgresDb`/`MariaDb` exactly as before.
mod rendered_sql;
pub use rendered_sql::RenderedSql;

mod rows_affected;
pub use rows_affected::RowsAffected;

mod sql_backend;
pub use sql_backend::SqlBackend;

mod sql_store;
pub use sql_store::SqlStore;
