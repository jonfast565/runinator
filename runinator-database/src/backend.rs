//! the per-backend seam.
//!
//! a backend exposes only what genuinely differs between databases: the concrete sqlx pool and the
//! sql dialect. every `DatabaseImpl` method body is written once as a blanket impl over `SqlBackend`
//! in `crate::operations`, so adding a database means implementing this trait, not re-typing queries.

use std::future::Future;

use runinator_models::errors::SendableError;
use sqlx::{
    Database, Pool, mysql::MySqlQueryResult, postgres::PgQueryResult, sqlite::SqliteQueryResult,
};

use crate::queries::{self, SqlDialect};

/// portable access to a statement's affected-row count.
///
/// each driver exposes `rows_affected` as an inherent method on its own `QueryResult`; this trait
/// lets generic code read it through `Database::QueryResult`.
pub trait RowsAffected {
    fn affected(&self) -> u64;
}

impl RowsAffected for SqliteQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

impl RowsAffected for PgQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

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

    /// the sql dialect used to render queries.
    fn dialect(&self) -> SqlDialect;

    /// render a `?`-placeholder template for this backend's dialect.
    fn render(&self, sql: &str) -> String {
        queries::render(self.dialect(), sql)
    }

    /// run embedded migrations and any extra init scripts.
    ///
    /// migrations are embedded per backend (the `sqlx::migrate!` macro is dir-specific), so this
    /// stays backend-owned rather than living in the generic operations blanket impl.
    fn init(&self, paths: &[String]) -> impl Future<Output = Result<(), SendableError>> + Send;
}
