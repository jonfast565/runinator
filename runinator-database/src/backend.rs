//! the per-backend seam.
//!
//! a backend exposes only what genuinely differs between databases: the concrete sqlx pool and the
//! sql dialect. every `DatabaseImpl` method body is written once as a blanket impl over `SqlBackend`
//! in `crate::operations`, so adding a database means implementing this trait, not re-typing queries.

use std::{future::Future, time::Duration};

use log::warn;
use runinator_models::errors::SendableError;
#[cfg(feature = "mysql")]
use sqlx::mysql::MySqlQueryResult;
#[cfg(feature = "postgres")]
use sqlx::postgres::PgQueryResult;
#[cfg(feature = "sqlite")]
use sqlx::sqlite::SqliteQueryResult;
use sqlx::{Database, Pool};

use crate::queries::SqlDialect;

const DELETE_RETRY_LIMIT: usize = 4;
const DELETE_RETRY_BASE_DELAY: Duration = Duration::from_millis(10);

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
pub trait RowsAffected {
    fn affected(&self) -> u64;
}

#[cfg(feature = "sqlite")]
impl RowsAffected for SqliteQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

#[cfg(feature = "postgres")]
impl RowsAffected for PgQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

#[cfg(feature = "mysql")]
impl RowsAffected for MySqlQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

/// the connection + dialect a generic database operation runs against.
pub trait SqlBackend: Send + Sync + 'static {
    /// the concrete sqlx database driver.
    type Db: Database;

    /// the pool generic operations execute against.
    fn pool(&self) -> &Pool<Self::Db>;

    /// Rebuild this backend around an already-connected pool. Pack imports use this to create an
    /// isolated single-connection pool whose connection remains inside one outer transaction.
    fn from_pool(pool: Pool<Self::Db>) -> Self;

    /// the sql dialect used to render queries.
    fn dialect(&self) -> SqlDialect;

    /// render a `?`-placeholder template for this backend's dialect.
    fn render(&self, sql: &str) -> String {
        self.dialect().render(sql)
    }

    /// run embedded bootstrap work and any extra init scripts.
    ///
    /// sql bootstrap files are embedded per backend (the `sqlx::migrate!` macro is dir-specific),
    /// so this stays backend-owned rather than living in the generic operations blanket impl.
    fn init(&self, paths: &[String]) -> impl Future<Output = Result<(), SendableError>> + Send;
}

/// a backend wearing the `DatabaseImpl` contract.
///
/// `DatabaseImpl` is defined in `runinator-store`, which is deliberately sqlx-free, so the orphan
/// rule forbids implementing it on a bare type parameter here. wrapping the backend in a type this
/// crate owns restores that: the 200-plus method bodies in `crate::operations` stay written once,
/// generically, rather than once per driver.
///
/// each driver module aliases this (`pub type SqliteDb = SqlStore<SqliteBackend>`) and supplies its
/// own `new`, so callers keep naming `SqliteDb`/`PostgresDb`/`MySqlDb` exactly as before.
pub struct SqlStore<B: SqlBackend> {
    backend: B,
}

impl<B: SqlBackend> SqlStore<B> {
    /// wrap an already-connected backend. driver modules expose a `new` that connects first; this
    /// is the seam a test or a caller with its own pool uses.
    pub fn from_backend(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }
}

impl<B: SqlBackend> SqlBackend for SqlStore<B> {
    type Db = B::Db;

    fn pool(&self) -> &Pool<Self::Db> {
        self.backend.pool()
    }

    fn from_pool(pool: Pool<Self::Db>) -> Self {
        SqlStore::from_backend(B::from_pool(pool))
    }

    fn dialect(&self) -> SqlDialect {
        self.backend.dialect()
    }

    fn init(&self, paths: &[String]) -> impl Future<Output = Result<(), SendableError>> + Send {
        self.backend.init(paths)
    }
}
