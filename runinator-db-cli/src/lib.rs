//! shared CLI-side helpers for selecting and constructing a runinator database backend.

use std::path::PathBuf;

use clap::ValueEnum;

#[cfg(feature = "mysql")]
pub use runinator_database::mysql::MySqlDb;
#[cfg(feature = "postgres")]
pub use runinator_database::postgres::PostgresDb;
#[cfg(feature = "sqlite")]
pub use runinator_database::sqlite::SqliteDb;

/// database backend selected by a CLI flag (also reads `RUNINATOR_DATABASE`).
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DatabaseBackend {
    Sqlite,
    Postgres,
    /// MySQL or MariaDB.
    #[value(alias = "mariadb")]
    Mysql,
}

impl DatabaseBackend {
    /// Stable backend label for logs and replica metadata.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }
}

/// Resolve the non-SQLite URL, returning the same user-facing error for every executable.
pub fn required_database_url(
    database_url: Option<String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    database_url.ok_or_else(|| {
        "--database-url must be provided when --database=postgres/mysql/mariadb".into()
    })
}

/// Ensure the parent directory of a SQLite path exists and return the connection string.
pub async fn prepare_sqlite_path(path: PathBuf) -> Result<String, std::io::Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(path.to_string_lossy().into_owned())
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
            #[cfg(feature = "sqlite")]
            $crate::DatabaseBackend::Sqlite => {
                let __conn: String = $sqlite;
                let $db = ::std::sync::Arc::new($crate::SqliteDb::new(&__conn).await?);
                $body
            }
            #[cfg(feature = "postgres")]
            $crate::DatabaseBackend::Postgres => {
                let __conn: String = $url;
                let $db = ::std::sync::Arc::new($crate::PostgresDb::new(&__conn).await?);
                $body
            }
            #[cfg(feature = "mysql")]
            $crate::DatabaseBackend::Mysql => {
                let __conn: String = $url;
                let $db = ::std::sync::Arc::new($crate::MySqlDb::new(&__conn).await?);
                $body
            }
            other => {
                return Err(format!(
                    "database backend '{other:?}' is not compiled into this binary"
                )
                .into());
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{DatabaseBackend, required_database_url};

    #[test]
    fn database_backend_labels_are_stable() {
        assert_eq!(DatabaseBackend::Sqlite.label(), "sqlite");
        assert_eq!(DatabaseBackend::Postgres.label(), "postgres");
        assert_eq!(DatabaseBackend::Mysql.label(), "mysql");
    }

    #[test]
    fn required_url_accepts_and_rejects_expected_inputs() {
        assert_eq!(
            required_database_url(Some("postgres://db".into())).unwrap(),
            "postgres://db"
        );
        assert!(required_database_url(None).is_err());
    }
}
