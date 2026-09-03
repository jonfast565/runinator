//! Pack imports execute through a private one-connection pool so every role operation shares one
//! outer transaction. These tests pin both ordinary statements and role methods that open their own
//! transactions: the latter must become savepoints rather than committing outside the pack.

use super::*;
use runinator_models::functions::{
    FunctionArtifact, FunctionRuntimeSpec, NewFunctionExport, NewFunctionPackage,
    NewFunctionVersion,
};

async fn database(label: &str) -> (SqliteDb, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-pack-transaction-{label}-{}.db",
        Uuid::now_v7()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

#[tokio::test]
async fn pack_transaction_commit_makes_mutations_visible() {
    let (db, path) = database("commit").await;
    let transaction = db.begin_pack_transaction().await.unwrap();

    transaction
        .upsert_setting(
            None,
            SettingKind::Config,
            "acme.shared".into(),
            "region".into(),
            b"us-east-1".to_vec(),
            100,
        )
        .await
        .unwrap();
    transaction.commit_pack_transaction().await.unwrap();

    let setting = db
        .fetch_setting(
            None,
            SettingKind::Config,
            "acme.shared".into(),
            "region".into(),
        )
        .await
        .unwrap()
        .expect("committed setting");
    assert_eq!(setting.value, b"us-east-1");

    drop(transaction);
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn pack_transaction_rollback_discards_mutations() {
    let (db, path) = database("rollback").await;
    let transaction = db.begin_pack_transaction().await.unwrap();

    transaction
        .upsert_setting(
            None,
            SettingKind::Secret,
            "acme.shared".into(),
            "token".into(),
            b"ciphertext".to_vec(),
            100,
        )
        .await
        .unwrap();
    transaction.rollback_pack_transaction().await.unwrap();

    assert!(
        db.fetch_setting(
            None,
            SettingKind::Secret,
            "acme.shared".into(),
            "token".into(),
        )
        .await
        .unwrap()
        .is_none()
    );

    drop(transaction);
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn nested_role_transaction_is_rolled_back_with_the_pack() {
    let (db, path) = database("nested-rollback").await;
    let digest = format!("sha256:{}", "1".repeat(64));
    db.upsert_function_artifact(&FunctionArtifact {
        digest: digest.clone(),
        size_bytes: 1,
        uri: "blob://runinator-function-artifacts/sha256/archive.zip".into(),
        media_type: "application/zip".into(),
        created_at: Utc::now(),
    })
    .await
    .unwrap();

    let transaction = db.begin_pack_transaction().await.unwrap();
    transaction
        .publish_function_version(&NewFunctionVersion {
            package: NewFunctionPackage {
                name: "pdf".into(),
                namespace: Some("acme.shared".into()),
                description: None,
                org_id: None,
            },
            artifact_digest: digest,
            manifest: Value::Null,
            runtime: FunctionRuntimeSpec::new("python3.13"),
            exports: vec![NewFunctionExport {
                name: "render".into(),
                handler: "render".into(),
                description: None,
                input: Vec::new(),
                output: Vec::new(),
                limits: Default::default(),
            }],
            alias: Some("latest".into()),
        })
        .await
        .unwrap();
    transaction.rollback_pack_transaction().await.unwrap();

    assert!(
        db.fetch_function_package(None, Some("acme.shared"), "pdf")
            .await
            .unwrap()
            .is_none(),
        "the inner publish transaction must not escape the pack rollback"
    );

    drop(transaction);
    drop(db);
    let _ = std::fs::remove_file(path);
}
