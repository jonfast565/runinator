//! legacy platform reconciliation preserves resources and rolls back unsafe changes.
use super::*;

#[tokio::test]
async fn reconciliation_repairs_sources_ownership_and_dangling_rows() {
    let path =
        std::env::temp_dir().join(format!("runinator-implicit-platform-{}.db", Uuid::new_v4()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&[]).await.unwrap();
    let org = Uuid::new_v4();
    sqlx::query("INSERT INTO organizations (id,name,slug,disabled,created_at,updated_at) VALUES (?,'Platform','platform',false,0,0)")
        .bind(org).execute(db.pool()).await.unwrap();
    crate::dialect_parity::assert_platform_reconciliation(&db, org).await;
}
