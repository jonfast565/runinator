#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default)]
pub struct ProvisionSpec {
    /// a maintenance connection used to issue `CREATE DATABASE`. postgres and mariadb cannot
    /// create a database over a connection to that same database, so without this a missing
    /// database is reported rather than created.
    pub admin_connection: Option<String>,
    /// the database name to create. derived from the connection string when omitted.
    pub database: Option<String>,
}
