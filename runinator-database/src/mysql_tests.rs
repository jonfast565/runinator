// integration tests against live MySQL and MariaDB. the assertions are the shared parity body in
// `crate::dialect_parity` — ON DUPLICATE KEY upserts, UPDATE+SELECT claims, INSERT IGNORE, and
// reserved-word quoting are exactly what it covers. what this file owns is provisioning: each run
// creates a throwaway database and drops it afterwards, so the suite is independent.
//
// bring the engine up with runinator-database/tests/docker-compose.yml.

use super::*;
use crate::dialect_parity::assert_dialect_parity;
use runinator_store::DatabaseImpl;
use sqlx::{Connection, MySqlConnection};
use uuid::Uuid;

fn base_url(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .filter(|url| !url.trim().is_empty())
}

// Split a `.../dbname` URL into the server URL and database name.
fn split_url(url: &str) -> (String, String) {
    let (server, db) = url
        .rsplit_once('/')
        .expect("url must contain a database path");
    (server.to_string(), db.to_string())
}

async fn fresh_db(variable: &str) -> Option<(MySqlDb, String, String)> {
    let url = base_url(variable)?;
    let (server, _) = split_url(&url);
    let db = format!("runinator_test_{}", Uuid::new_v4().simple());
    let mut conn = MySqlConnection::connect(&server).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&mut conn)
        .await
        .unwrap();
    let db_url = format!("{server}/{db}");
    let pool = MySqlDb::new(&db_url).await.unwrap();
    pool.run_init_scripts(&Vec::new()).await.unwrap();
    Some((pool, server, db))
}

async fn drop_db(pool: MySqlDb, server: &str, db: &str) {
    pool.pool().close().await;
    let mut conn = MySqlConnection::connect(server).await.unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db}"))
        .execute(&mut conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn mariadb_full_lifecycle() {
    let Some((db, server, dbname)) = fresh_db("RUNINATOR_TEST_MARIADB_URL").await else {
        eprintln!("skipping: set RUNINATOR_TEST_MARIADB_URL to run MariaDB tests");
        return;
    };

    assert_dialect_parity(&db).await;

    drop_db(db, &server, &dbname).await;
}

#[tokio::test]
async fn mysql_full_lifecycle() {
    let Some((db, server, dbname)) = fresh_db("RUNINATOR_TEST_MYSQL_URL").await else {
        eprintln!("skipping: set RUNINATOR_TEST_MYSQL_URL to run MySQL tests");
        return;
    };

    assert_dialect_parity(&db).await;

    drop_db(db, &server, &dbname).await;
}
