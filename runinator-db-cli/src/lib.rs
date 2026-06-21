//! shared CLI-side helpers for selecting and constructing a runinator database backend.

use clap::ValueEnum;

pub use runinator_database::{mysql::MySqlDb, postgres::PostgresDb, sqlite::SqliteDb};

/// database backend selected by a CLI flag (also reads `RUNINATOR_DATABASE`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DatabaseBackend {
    Sqlite,
    Postgres,
    /// MySQL or MariaDB.
    #[value(alias = "mariadb")]
    Mysql,
}

/// construct the concrete database for `$backend`, bind it to `$db`, and run `$body`.
///
/// `sqlite` and `url` are connection-string expressions evaluated only in their matching
/// arm, so each arm can resolve (and error on) just the inputs it needs. `$body` is
/// expanded once per backend with `$db` bound to an `Arc<concrete db>`; it may use `.await`
/// and `?` from the surrounding async context.
#[macro_export]
macro_rules! dispatch_database {
    ($backend:expr, sqlite: $sqlite:expr, url: $url:expr, |$db:ident| $body:block) => {
        match $backend {
            $crate::DatabaseBackend::Sqlite => {
                let __conn: String = $sqlite;
                let $db = ::std::sync::Arc::new($crate::SqliteDb::new(&__conn).await?);
                $body
            }
            $crate::DatabaseBackend::Postgres => {
                let __conn: String = $url;
                let $db = ::std::sync::Arc::new($crate::PostgresDb::new(&__conn).await?);
                $body
            }
            $crate::DatabaseBackend::Mysql => {
                let __conn: String = $url;
                let $db = ::std::sync::Arc::new($crate::MySqlDb::new(&__conn).await?);
                $body
            }
        }
    };
}
