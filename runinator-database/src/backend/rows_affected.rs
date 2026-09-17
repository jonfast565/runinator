#[allow(unused_imports)]
use super::*;

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

#[cfg(feature = "mariadb")]
impl RowsAffected for MySqlQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}
