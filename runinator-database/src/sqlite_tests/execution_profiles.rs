use super::*;
use runinator_models::execution_profiles::{
    ExecutionProfile, ExecutionProfileAgentStatus, ExecutionProfileApprovalState,
    ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec, ExecutionProfileHealth,
    ExecutionProfileOperation, ExecutionProfileOperationKind, ExecutionProfileOperationState,
    ExecutionProfileRevision,
};

#[tokio::test]
async fn execution_profile_publications_are_atomic_and_org_scoped() {
    let path = std::env::temp_dir().join(format!(
        "runinator-execution-profiles-{}.db",
        Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let org_id = Uuid::new_v4();
    let id = Uuid::new_v4();
    let now = Utc::now();
    let profile = ExecutionProfile {
        id,
        org_id: Some(org_id),
        name: "aws-production".into(),
        description: "fixture".into(),
        credential_scopes: vec!["aws".into()],
        collection: ExecutionProfileCollectionSpec::default(),
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Unpublished,
        last_error: None,
        created_at: now,
        updated_at: now,
    };
    db.upsert_execution_profile(&profile).await.unwrap();
    assert!(db.list_execution_profiles(None).await.unwrap().is_empty());
    assert_eq!(
        db.list_execution_profiles(Some(org_id))
            .await
            .unwrap()
            .len(),
        1
    );

    let publisher_id = Uuid::new_v4();
    db.insert_execution_profile_revision(&ExecutionProfileRevision {
        profile_id: id,
        revision: 1,
        digest: "bundle".into(),
        size_bytes: 42,
        publisher_id: Some(publisher_id),
        expires_at: Some(now + Duration::hours(1)),
        created_at: now,
        uri: "blob://execution-profiles/profile/1.bundle".into(),
    })
    .await
    .unwrap();

    let published = db.fetch_execution_profile(id).await.unwrap().unwrap();
    assert_eq!(published.current_revision, Some(1));
    assert_eq!(published.current_digest.as_deref(), Some("bundle"));
    assert_eq!(published.current_publisher_id, Some(publisher_id));
    assert_eq!(published.health, ExecutionProfileHealth::Ready);
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn execution_profile_collection_statuses_are_per_digest_and_claimed_once() {
    let path = std::env::temp_dir().join(format!(
        "runinator-execution-profile-collection-statuses-{}.db",
        Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let id = Uuid::new_v4();
    let agent_id = Uuid::new_v4();
    let now = Utc::now();
    db.upsert_execution_profile(&ExecutionProfile {
        id,
        org_id: None,
        name: "github-default".into(),
        description: "fixture".into(),
        credential_scopes: vec!["github".into()],
        collection: ExecutionProfileCollectionSpec::default(),
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config-a".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Unpublished,
        last_error: None,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();

    db.upsert_execution_profile_agent_status(&ExecutionProfileAgentStatus {
        profile_id: id,
        agent_id,
        config_digest: "config-a".into(),
        approval: ExecutionProfileApprovalState::Approved,
        last_seen_at: now,
        last_attempt_at: Some(now),
        last_success_at: None,
        last_error: Some("missing file".into()),
    })
    .await
    .unwrap();
    db.upsert_execution_profile_agent_status(&ExecutionProfileAgentStatus {
        profile_id: id,
        agent_id,
        config_digest: "config-a".into(),
        approval: ExecutionProfileApprovalState::Approved,
        last_seen_at: now + Duration::seconds(1),
        last_attempt_at: Some(now + Duration::seconds(1)),
        last_success_at: Some(now + Duration::seconds(1)),
        last_error: None,
    })
    .await
    .unwrap();
    db.upsert_execution_profile_agent_status(&ExecutionProfileAgentStatus {
        profile_id: id,
        agent_id,
        config_digest: "config-b".into(),
        approval: ExecutionProfileApprovalState::ApprovalRequired,
        last_seen_at: now + Duration::seconds(2),
        last_attempt_at: None,
        last_success_at: None,
        last_error: None,
    })
    .await
    .unwrap();

    let current = db
        .list_execution_profile_agent_statuses(id, "config-a")
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert!(current[0].last_success_at.is_some());
    assert_eq!(current[0].last_error, None);
    let changed = db
        .list_execution_profile_agent_statuses(id, "config-b")
        .await
        .unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(
        changed[0].approval,
        ExecutionProfileApprovalState::ApprovalRequired
    );

    let operation = ExecutionProfileOperation {
        id: Uuid::new_v4(),
        profile_id: id,
        config_digest: "config-a".into(),
        kind: ExecutionProfileOperationKind::DryRun,
        state: ExecutionProfileOperationState::Queued,
        requested_at: now,
        requested_by: None,
        claimed_by: None,
        started_at: None,
        lease_expires_at: None,
        completed_at: None,
        error: None,
    };
    db.insert_execution_profile_operation(&operation)
        .await
        .unwrap();
    assert!(
        db.insert_execution_profile_operation(&ExecutionProfileOperation {
            id: Uuid::new_v4(),
            ..operation.clone()
        })
        .await
        .is_err()
    );
    assert_eq!(
        db.claim_execution_profile_operation(
            operation.id,
            agent_id,
            "config-a",
            now,
            now + Duration::minutes(30),
        )
        .await
        .unwrap()
        .unwrap()
        .state,
        ExecutionProfileOperationState::Running
    );
    assert!(
        db.claim_execution_profile_operation(
            operation.id,
            Uuid::new_v4(),
            "config-a",
            now,
            now + Duration::minutes(30),
        )
        .await
        .unwrap()
        .is_none()
    );
    let replacement_agent = Uuid::new_v4();
    assert_eq!(
        db.claim_execution_profile_operation(
            operation.id,
            replacement_agent,
            "config-a",
            now + Duration::minutes(31),
            now + Duration::minutes(61),
        )
        .await
        .unwrap()
        .unwrap()
        .claimed_by,
        Some(replacement_agent)
    );
    assert!(
        db.complete_execution_profile_operation(
            operation.id,
            replacement_agent,
            ExecutionProfileOperationState::Succeeded,
            None,
            now,
        )
        .await
        .unwrap()
    );
    assert_eq!(
        db.fetch_latest_execution_profile_operation(id, "config-a")
            .await
            .unwrap()
            .unwrap()
            .state,
        ExecutionProfileOperationState::Succeeded
    );
    drop(db);
    let _ = std::fs::remove_file(path);
}
