// integration tests against a live MariaDB/MySQL, gated on RUNINATOR_TEST_MYSQL_URL
// (e.g. mysql://root:runinator@127.0.0.1:53307/runinator). the assertions are the shared parity
// body in `crate::dialect_parity` — ON DUPLICATE KEY upserts, UPDATE+SELECT claims, INSERT IGNORE,
// and reserved-word quoting are exactly what it covers. what this file owns is provisioning: each
// run creates a throwaway database and drops it afterwards, so the suite is independent.
//
// bring the engine up with runinator-database/tests/docker-compose.yml.

use super::*;
use crate::dialect_parity::assert_dialect_parity;
use crate::interfaces::DatabaseImpl;
use sqlx::{Connection, MySqlConnection};
use uuid::Uuid;

fn base_url() -> Option<String> {
    std::env::var("RUNINATOR_TEST_MYSQL_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

// split a `.../dbname` url into (server-url-without-db, dbname).
fn split_url(url: &str) -> (String, String) {
    let (server, db) = url
        .rsplit_once('/')
        .expect("url must contain a database path");
    (server.to_string(), db.to_string())
}

async fn fresh_db() -> Option<(MySqlDb, String, String)> {
    let url = base_url()?;
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

async fn drop_db(server: &str, db: &str) {
    let mut conn = MySqlConnection::connect(server).await.unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db}"))
        .execute(&mut conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn mariadb_full_lifecycle() {
    let Some((db, server, dbname)) = fresh_db().await else {
        eprintln!("skipping: set RUNINATOR_TEST_MYSQL_URL to run MariaDB tests");
        return;
    };

    assert_dialect_parity(&db).await;

    drop_db(&server, &dbname).await;
}
