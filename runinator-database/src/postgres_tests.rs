// integration tests against a live PostgreSQL, gated on RUNINATOR_TEST_POSTGRES_URL
// (e.g. postgres://runi:runi@127.0.0.1:55433/runi). the assertions are the shared parity body in
// `crate::dialect_parity`, so what this file owns is provisioning: each run creates a throwaway
// database and drops it afterwards, which keeps the suite independent and re-runnable.
//
// bring the engine up with runinator-database/tests/docker-compose.yml.

use super::*;
use crate::dialect_parity::assert_dialect_parity;
use crate::interfaces::DatabaseImpl;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

fn base_url() -> Option<String> {
    std::env::var("RUNINATOR_TEST_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

// Split a `.../dbname` URL into the server URL and database name. Unlike MySQL, PostgreSQL has no
// "no database selected" connection, so the original database is kept as the maintenance one to
// issue CREATE/DROP from.
fn split_url(url: &str) -> (String, String) {
    let (server, db) = url
        .rsplit_once('/')
        .expect("url must contain a database path");
    (server.to_string(), db.to_string())
}

async fn fresh_db() -> Option<(PostgresDb, String, String)> {
    let url = base_url()?;
    let (server, _) = split_url(&url);
    let db = format!("runinator_test_{}", Uuid::new_v4().simple());

    let mut conn = PgConnection::connect(&url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.ok();

    let pool = PostgresDb::new(&format!("{server}/{db}")).await.unwrap();
    pool.run_init_scripts(&Vec::new()).await.unwrap();
    Some((pool, url, db))
}

async fn drop_db(pool: PostgresDb, maintenance_url: &str, db: &str) {
    // postgres refuses to drop a database with sessions still attached, so the pool goes first.
    pool.pool().close().await;
    let mut conn = PgConnection::connect(maintenance_url).await.unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.ok();
}

#[tokio::test]
async fn postgres_full_lifecycle() {
    let Some((db, maintenance_url, dbname)) = fresh_db().await else {
        eprintln!("skipping: set RUNINATOR_TEST_POSTGRES_URL to run PostgreSQL tests");
        return;
    };

    assert_dialect_parity(&db).await;

    drop_db(db, &maintenance_url, &dbname).await;
}
