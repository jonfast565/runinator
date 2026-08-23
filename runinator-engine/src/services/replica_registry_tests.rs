//! replica-registry coordination over the real narrow store role.

use std::{path::PathBuf, sync::Arc};

use chrono::{Duration, Utc};
use runinator_comm::AgentDirectiveKind;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    auth::AuthContext,
    json,
    replicas::{ReplicaKind, ReplicaRegistrationRequest},
};
use runinator_store::{DatabaseImpl, roles::ReplicaStore};

use super::*;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-replica-registry-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

fn registration(instance_id: &str, runtime_id: &str) -> ReplicaRegistrationRequest {
    ReplicaRegistrationRequest {
        replica_type: ReplicaKind::Worker,
        instance_id: instance_id.to_string(),
        runtime_id: runtime_id.to_string(),
        display_name: Some(instance_id.to_string()),
        host: None,
        port: None,
        base_path: None,
        version: None,
        attributes: json!({}),
    }
}

#[tokio::test]
async fn directive_is_durable_before_its_transport_layer_observes_it() {
    let (db, path) = test_db().await;
    let registry = ReplicaRegistry::new(db.clone());
    let replica = registry
        .register(
            registration("registry-test", "runtime-a"),
            None,
            &AuthContext::disabled_platform_admin(),
        )
        .await
        .unwrap();
    let directive = registry
        .issue_directive(
            replica.replica_id,
            AgentDirectiveKind::Diagnostics,
            Utc::now() + Duration::minutes(5),
        )
        .await
        .unwrap()
        .expect("the registered replica accepts a directive");

    let rows = db
        .list_agent_directives(replica.replica_id, 10)
        .await
        .unwrap();
    assert!(
        rows.iter()
            .any(|row| row.directive_id == directive.directive_id)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn missing_replica_returns_no_directive() {
    let (db, path) = test_db().await;
    let registry = ReplicaRegistry::new(db);
    let directive = registry
        .issue_directive(
            uuid::Uuid::new_v4(),
            AgentDirectiveKind::Diagnostics,
            Utc::now() + Duration::minutes(5),
        )
        .await
        .unwrap();

    assert!(directive.is_none());
    let _ = std::fs::remove_file(path);
}
