use super::*;
use runinator_models::{
    orchestration::{IngressAdmission, IngressAdmissionStatus, IngressTarget, IngressTargetKind},
    workspaces::{NewWorkspaceLease, WorkspaceStatus},
};

#[tokio::test]
async fn workspace_allocation_and_transitions_are_idempotent_and_cas_guarded() {
    let path = std::env::temp_dir().join(format!("runinator-workspaces-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let now = Utc::now();
    let workflow_id = db
        .insert_workflow(&workflow("workspace-target"))
        .await
        .unwrap()
        .id
        .unwrap();
    let admission_id = Uuid::now_v7();
    let admission = db
        .claim_ingress_admission(
            IngressAdmission {
                id: Some(admission_id),
                org_id: None,
                scope: "jobs".into(),
                correlation_key: "job-42".into(),
                generation: 1,
                target: IngressTarget {
                    kind: IngressTargetKind::Workflow,
                    id: workflow_id,
                },
                status: IngressAdmissionStatus::Active,
                workflow_run_id: None,
                pipeline_run_id: None,
                policy: Value::Null,
                created_at: now,
                updated_at: now,
            },
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        admission,
        runinator_models::orchestration::IngressAdmissionClaim::Acquired(_)
    ));

    let workspace_id = Uuid::now_v7();
    let new_workspace = NewWorkspaceLease {
        id: workspace_id,
        admission_id,
        generation: 1,
        scope: "processing".into(),
        attempt: 1,
        worker_instance_id: "worker-a".into(),
        worker_replica_id: Some(Uuid::now_v7()),
        local_key: "admissions/job-42/1".into(),
        requirements: runinator_models::json!({
            "labels": {"pool": "local"}
        }),
        leased_until: now + Duration::minutes(15),
    };
    let first = db.allocate_workspace(new_workspace.clone()).await.unwrap();
    let duplicate = db
        .allocate_workspace(NewWorkspaceLease {
            id: Uuid::now_v7(),
            ..new_workspace
        })
        .await
        .unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(first.version, 1);
    assert_eq!(first.status, WorkspaceStatus::Allocating);
    let listed = db
        .fetch_workspaces_for_admission(admission_id, 1)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, workspace_id);
    assert_eq!(
        first
            .requirements
            .pointer("/labels/pool")
            .and_then(Value::as_str),
        Some("local")
    );

    assert!(
        db.transition_workspace_cas(
            workspace_id,
            1,
            WorkspaceStatus::Allocating,
            WorkspaceStatus::Active,
            None,
            now,
        )
        .await
        .unwrap()
    );
    assert!(
        !db.transition_workspace_cas(
            workspace_id,
            1,
            WorkspaceStatus::Allocating,
            WorkspaceStatus::Active,
            None,
            now,
        )
        .await
        .unwrap()
    );
    let active = db.fetch_workspace(workspace_id).await.unwrap().unwrap();
    assert_eq!(active.version, 2);
    assert_eq!(active.status, WorkspaceStatus::Active);

    let rebound_replica = Uuid::now_v7();
    assert!(
        db.renew_workspace(
            workspace_id,
            2,
            "worker-a".into(),
            Some(rebound_replica),
            now + Duration::minutes(30),
            now,
        )
        .await
        .unwrap()
    );
    let rebound = db.fetch_workspace(workspace_id).await.unwrap().unwrap();
    assert_eq!(
        rebound.version, 2,
        "renewal must not invalidate frozen affinity tokens"
    );
    assert_eq!(rebound.worker_replica_id, Some(rebound_replica));

    assert!(
        db.transition_workspace_cas(
            workspace_id,
            2,
            WorkspaceStatus::Active,
            WorkspaceStatus::Finalizing,
            None,
            now,
        )
        .await
        .unwrap()
    );
    let evidence = runinator_models::json!({"reason": "complete"});
    assert!(
        db.transition_workspace_cas(
            workspace_id,
            3,
            WorkspaceStatus::Finalizing,
            WorkspaceStatus::Released,
            Some(evidence.clone()),
            now,
        )
        .await
        .unwrap()
    );
    let released = db.fetch_workspace(workspace_id).await.unwrap().unwrap();
    assert_eq!(released.version, 4);
    assert_eq!(released.status, WorkspaceStatus::Released);
    assert_eq!(released.evidence, evidence);
    assert!(
        db.fetch_expired_workspaces(now + Duration::hours(1), 10)
            .await
            .unwrap()
            .is_empty(),
        "terminal workspaces are never recovered"
    );

    let abandoned_id = Uuid::now_v7();
    let abandoned = db
        .allocate_workspace(NewWorkspaceLease {
            id: abandoned_id,
            admission_id,
            generation: 1,
            scope: "recovery".into(),
            attempt: 1,
            worker_instance_id: "worker-lost".into(),
            worker_replica_id: None,
            local_key: "admissions/job-42/recovery/1".into(),
            requirements: Value::Null,
            leased_until: now,
        })
        .await
        .unwrap();
    assert!(
        db.transition_workspace_cas(
            abandoned_id,
            abandoned.version,
            WorkspaceStatus::Allocating,
            WorkspaceStatus::Abandoned,
            Some(runinator_models::json!({ "reason": "worker lost" })),
            now,
        )
        .await
        .unwrap()
    );
    let pending_notifications = db.fetch_abandoned_workspaces(10).await.unwrap();
    assert_eq!(pending_notifications.len(), 1);
    assert_eq!(pending_notifications[0].id, abandoned_id);
    assert!(
        db.mark_workspace_abandonment_notified(
            abandoned_id,
            pending_notifications[0].version,
            now,
        )
        .await
        .unwrap()
    );
    assert!(db.fetch_abandoned_workspaces(10).await.unwrap().is_empty());
}
