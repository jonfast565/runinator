//! adapter secret-binding tenancy: an adapter resolves its own organization's secrets and
//! platform-scoped ones, and never another organization's.

use super::adapter_operations::AdapterOperations;
use runinator_database::sqlite::SqliteDb;
use runinator_models::settings::SettingKind;
use runinator_secrets::{secret_cipher::SecretCipher, stored_secret::StoredSecret};
use runinator_store::{DatabaseImpl, RuntimeStore, roles::SettingStore};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use uuid::Uuid;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-adapter-operations-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

/// store a secret owned by `org_id` (`None` is platform scope) and return its durable id.
async fn store_secret(db: &Arc<SqliteDb>, org_id: Option<Uuid>, scope: &str, name: &str) -> Uuid {
    let cipher = SecretCipher::from_env();
    let encoded = StoredSecret::new("s3cret".into(), None).encode().unwrap();
    db.upsert_setting(
        org_id,
        SettingKind::Secret,
        scope.into(),
        name.into(),
        cipher.encrypt(&encoded),
        1,
    )
    .await
    .unwrap();
    db.list_settings(org_id)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.scope == scope && record.name == name)
        .expect("stored secret")
        .id
}

fn bindings(id: Uuid) -> BTreeMap<String, Uuid> {
    BTreeMap::from([("api_token".to_string(), id)])
}

#[tokio::test]
async fn resolves_a_secret_owned_by_the_adapter_organization() {
    let (db, path) = test_db().await;
    let org_id = Uuid::new_v4();
    // the authored namespace is "jira", which is not a tenancy key; tenancy is `org_id`.
    let id = store_secret(&db, Some(org_id), "jira", "token").await;

    let resolved = AdapterOperations::new(db.clone())
        .resolve_secrets(org_id, &bindings(id))
        .await
        .expect("an organization's own secret resolves");
    assert_eq!(resolved["api_token"], serde_json::json!("s3cret"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn resolves_a_platform_scoped_secret_for_any_organization() {
    let (db, path) = test_db().await;
    // platform scope is shared infrastructure, and the org listing excludes it, so this only
    // works because resolution falls back to the platform scope.
    let id = store_secret(&db, None, "jira", "token").await;

    let resolved = AdapterOperations::new(db.clone())
        .resolve_secrets(Uuid::new_v4(), &bindings(id))
        .await
        .expect("a platform secret resolves for an organization-scoped adapter");
    assert_eq!(resolved["api_token"], serde_json::json!("s3cret"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn rejects_a_secret_owned_by_another_organization() {
    let (db, path) = test_db().await;
    let owner = Uuid::new_v4();
    let id = store_secret(&db, Some(owner), "jira", "token").await;

    let error = AdapterOperations::new(db.clone())
        .resolve_secrets(Uuid::new_v4(), &bindings(id))
        .await
        .expect_err("another organization's secret stays invisible");
    assert!(
        error.contains("does not exist") || error.contains("outside the adapter organization"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_file(path);
}
