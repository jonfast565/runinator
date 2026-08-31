//! replica-registry coordination over the real narrow store role.

use std::{path::PathBuf, sync::Arc};

use chrono::{Duration, Utc};
use runinator_comm::{AgentDirectiveKind, ReplicaAvailability};
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
        replica_id: None,
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

#[tokio::test]
async fn kicked_replica_is_no_longer_owned_by_its_agent_principal() {
    let (db, path) = test_db().await;
    let registry = ReplicaRegistry::new(db);
    let principal_id = uuid::Uuid::new_v4();
    let context = AuthContext {
        principal_id: Some(principal_id),
        session_id: None,
        kind: runinator_models::auth::PrincipalKind::Service,
        platform_role: None,
        assignments: Vec::new(),
        system_role: Some(runinator_models::rbac::SystemRole::Agent),
        action_ceiling: Vec::new(),
        org_id: None,
    };
    let replica = registry
        .register(registration("kick-test", "runtime-a"), None, &context)
        .await
        .unwrap();

    assert!(
        registry
            .agent_owns_replica(&context, replica.replica_id)
            .await
            .unwrap()
    );
    registry.kick(replica.replica_id).await.unwrap();
    assert!(
        !registry
            .agent_owns_replica(&context, replica.replica_id)
            .await
            .unwrap()
    );
    assert!(
        !registry
            .agent_owns_runtime_registration(&context, &registration("kick-test", "runtime-a"))
            .await
            .unwrap()
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn broker_availability_refuses_webservice_registration() {
    let (db, path) = test_db().await;
    let registry = ReplicaRegistry::new(db);
    let mut request = registration("webservice", "runtime-a");
    request.replica_id = Some(uuid::Uuid::now_v7());
    request.replica_type = ReplicaKind::Webservice;

    let err = registry
        .observe_broker_availability(ReplicaAvailability::Available {
            registration: request,
            providers: Vec::new(),
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("register directly"));
    let _ = std::fs::remove_file(path);
}
