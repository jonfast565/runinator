//! replica-registry coordination over the real narrow store role.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::{Duration, Utc};
use runinator_comm::AgentDirectiveKind;
use runinator_database::{
    interfaces::{DatabaseImpl, ReplicaStore},
    sqlite::SqliteDb,
};
use runinator_models::{
    auth::AuthContext,
    json,
    replicas::{ReplicaKind, ReplicaRegistrationRequest},
};

use super::*;

#[derive(Default)]
struct RecordingEvents {
    queued_directives: AtomicUsize,
}

impl ReplicaRegistryEvents for RecordingEvents {
    fn agent_directive_queued(&self) {
        self.queued_directives.fetch_add(1, Ordering::Relaxed);
    }
}

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
async fn directive_is_durable_before_the_service_signals_its_transport_port() {
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
    let events = RecordingEvents::default();

    let directive = registry
        .issue_directive(
            replica.replica_id,
            AgentDirectiveKind::Diagnostics,
            Utc::now() + Duration::minutes(5),
            &events,
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
    assert_eq!(events.queued_directives.load(Ordering::Relaxed), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn missing_replica_does_not_signal_a_directive_delivery() {
    let (db, path) = test_db().await;
    let registry = ReplicaRegistry::new(db);
    let events = RecordingEvents::default();

    let directive = registry
        .issue_directive(
            uuid::Uuid::new_v4(),
            AgentDirectiveKind::Diagnostics,
            Utc::now() + Duration::minutes(5),
            &events,
        )
        .await
        .unwrap();

    assert!(directive.is_none());
    assert_eq!(events.queued_directives.load(Ordering::Relaxed), 0);

    let _ = std::fs::remove_file(path);
}
