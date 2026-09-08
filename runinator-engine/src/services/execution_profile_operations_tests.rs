//! execution-profile configuration and event publication.

use super::*;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    execution_profiles::{
        ExecutionProfileBinding, ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec,
    },
    orchestration::{AdapterAuthentication, AdapterTransport},
};
use runinator_store::{DatabaseImpl, roles::NewAdapterDefinition};

fn request() -> ExecutionProfilePutRequest {
    ExecutionProfilePutRequest {
        name: " github-default ".into(),
        description: " GitHub login ".into(),
        credential_scopes: vec!["github".into(), " copilot ".into()],
        collection: ExecutionProfileCollectionSpec {
            sources: vec![ExecutionProfileSource::File {
                path: " ~/.gitconfig ".into(),
                target: " .gitconfig ".into(),
            }],
            ..Default::default()
        },
        exposure: ExecutionProfileExposureSpec {
            home_overlay: true,
            environment: BTreeMap::from([(
                " GH_CONFIG_DIR ".into(),
                " ${PROFILE_HOME}/.config/gh ".into(),
            )]),
            ..Default::default()
        },
        enabled: true,
    }
}

#[test]
fn canonical_profile_normalization_is_stable() {
    let normalized = normalize_profile(request()).expect("valid profile");
    assert_eq!(normalized.name, "github-default");
    assert_eq!(normalized.description, "GitHub login");
    assert_eq!(normalized.credential_scopes, ["copilot", "github"]);
    assert_eq!(
        normalized.exposure.environment.get("GH_CONFIG_DIR"),
        Some(&"${PROFILE_HOME}/.config/gh".to_string())
    );
    assert_eq!(
        normalize_profile(normalized.clone()).expect("idempotent"),
        normalized
    );
}

#[test]
fn canonical_profile_rejects_ambiguous_scopes_and_targets() {
    let mut duplicate_scope = request();
    duplicate_scope.credential_scopes = vec!["GitHub".into(), "github".into()];
    assert!(normalize_profile(duplicate_scope).is_err());

    let mut duplicate_target = request();
    duplicate_target
        .collection
        .sources
        .push(ExecutionProfileSource::File {
            path: "~/.config/gh".into(),
            target: ".gitconfig".into(),
        });
    assert!(normalize_profile(duplicate_target).is_err());
}

#[tokio::test]
async fn unchanged_configuration_preserves_publication_and_changes_invalidate_it() {
    let path = std::env::temp_dir().join(format!("runinator-profile-{}.db", Uuid::now_v7()));
    let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let service = ExecutionProfileOperations::new(db);
    let id = Uuid::new_v4();
    let configured = service
        .configure(id, None, request(), Some(Utc::now()), true)
        .await
        .unwrap();
    service
        .publish_revision(&ExecutionProfileRevision {
            profile_id: id,
            revision: 1,
            digest: "archive".into(),
            size_bytes: 7,
            publisher_id: None,
            expires_at: None,
            created_at: configured.updated_at,
            uri: "blob://profile".into(),
        })
        .await
        .unwrap();

    let unchanged = service
        .configure(
            id,
            None,
            request(),
            Some(configured.updated_at + chrono::Duration::seconds(1)),
            true,
        )
        .await
        .unwrap();
    assert_eq!(unchanged.config_version, 1);
    assert_eq!(unchanged.current_revision, Some(1));

    let mut changed_request = request();
    changed_request.description = "Changed".into();
    let changed = service
        .configure(
            id,
            None,
            changed_request,
            Some(configured.updated_at + chrono::Duration::seconds(2)),
            true,
        )
        .await
        .unwrap();
    assert_eq!(changed.config_version, 2);
    assert_eq!(changed.health, ExecutionProfileHealth::Unpublished);
    assert_eq!(changed.current_revision, None);
    assert_eq!(changed.current_digest, None);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn scoped_listing_includes_platform_profiles() {
    let path = std::env::temp_dir().join(format!("runinator-profile-{}.db", Uuid::now_v7()));
    let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let service = ExecutionProfileOperations::new(db);
    let org_id = Uuid::now_v7();

    let mut platform = request();
    platform.name = "platform-profile".into();
    service
        .configure(Uuid::now_v7(), None, platform, Some(Utc::now()), true)
        .await
        .unwrap();
    let mut organization = request();
    organization.name = "organization-profile".into();
    service
        .configure(
            Uuid::now_v7(),
            Some(org_id),
            organization,
            Some(Utc::now()),
            true,
        )
        .await
        .unwrap();

    let profiles = service.list_visible_in_scope(Some(org_id)).await.unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].name, "organization-profile");
    assert_eq!(profiles[1].name, "platform-profile");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn adapter_dependencies_are_reported_for_profile_deletion() {
    let path = std::env::temp_dir().join(format!("runinator-profile-{}.db", Uuid::now_v7()));
    let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let service = ExecutionProfileOperations::new(db.clone());
    let org_id = Uuid::now_v7();
    let profile_id = Uuid::now_v7();
    let profile = service
        .configure(profile_id, Some(org_id), request(), Some(Utc::now()), true)
        .await
        .unwrap();

    db.create_orchestration_adapter(
        NewAdapterDefinition {
            id: Uuid::now_v7(),
            org_id,
            name: "github-poller".into(),
            kind: "github".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Polling,
            endpoint_identity: Uuid::now_v7().to_string(),
            configuration: runinator_models::json!({}),
            authentication: AdapterAuthentication::ExecutionProfile {
                profile: ExecutionProfileBinding::resolved(profile_id, &profile.name),
                required_labels: BTreeMap::new(),
                required_scopes: vec!["github".into()],
            },
            identity_configuration: runinator_models::json!({}),
            actor_id: None,
        },
        Utc::now(),
    )
    .await
    .unwrap();

    assert_eq!(
        service
            .dependent_adapter_names(profile_id, Some(org_id), &profile.name)
            .await
            .unwrap(),
        ["github-poller"]
    );

    let _ = std::fs::remove_file(path);
}

async fn expect_profile_event(broker: &impl runinator_broker_core::Broker, org_id: Option<Uuid>) {
    let delivery = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        broker.receive_event("profile-test"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivery.event.org_id, org_id);
    assert!(matches!(
        delivery.event.kind,
        UiEventKind::ExecutionProfilesChanged
    ));
}

#[tokio::test]
async fn collection_lifecycle_emits_scoped_events_after_persisting_state() {
    use runinator_broker_core::{Broker, in_memory::InMemoryBroker};
    use runinator_models::execution_profiles::{
        ExecutionProfileApprovalState, ExecutionProfileOperationKind,
        ExecutionProfileOperationState,
    };
    let path = std::env::temp_dir().join(format!("runinator-profile-events-{}.db", Uuid::now_v7()));
    let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let broker = Arc::new(InMemoryBroker::new());
    // register the fan-out subscriber before the first mutation emits an event.
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            broker.receive_event("profile-test")
        )
        .await
        .is_err()
    );
    let service =
        ExecutionProfileOperations::new(db).with_events(UiEventPublisher::new(broker.clone()));
    let org_id = Some(Uuid::now_v7());
    let profile = service
        .configure(Uuid::now_v7(), org_id, request(), None, true)
        .await
        .unwrap();
    expect_profile_event(broker.as_ref(), org_id).await;
    let agent_id = Uuid::now_v7();
    service
        .record_agent_status(&ExecutionProfileAgentStatus {
            profile_id: profile.id,
            agent_id,
            config_digest: profile.config_digest.clone(),
            approval: ExecutionProfileApprovalState::ApprovalRequired,
            last_seen_at: Utc::now(),
            last_attempt_at: None,
            last_success_at: None,
            last_error: None,
        })
        .await
        .unwrap();
    expect_profile_event(broker.as_ref(), org_id).await;
    assert_eq!(
        service.collection_status(&profile).await.unwrap().agents[0].approval,
        ExecutionProfileApprovalState::ApprovalRequired
    );
    let operation = service
        .request_operation(&ExecutionProfileOperation {
            id: Uuid::now_v7(),
            profile_id: profile.id,
            config_digest: profile.config_digest.clone(),
            kind: ExecutionProfileOperationKind::Refresh,
            state: ExecutionProfileOperationState::Queued,
            requested_at: Utc::now(),
            requested_by: None,
            claimed_by: None,
            started_at: None,
            lease_expires_at: None,
            completed_at: None,
            error: None,
        })
        .await
        .unwrap();
    expect_profile_event(broker.as_ref(), org_id).await;
    service
        .claim_operation(operation.id, agent_id, &profile.config_digest)
        .await
        .unwrap()
        .unwrap();
    expect_profile_event(broker.as_ref(), org_id).await;
    assert!(
        service
            .complete_operation(
                operation.id,
                agent_id,
                org_id,
                ExecutionProfileOperationCompleteRequest {
                    state: ExecutionProfileOperationState::Failed,
                    error: Some("collector failed".into()),
                }
            )
            .await
            .unwrap()
    );
    expect_profile_event(broker.as_ref(), org_id).await;
    assert_eq!(
        service
            .collection_status(&profile)
            .await
            .unwrap()
            .latest_operation
            .unwrap()
            .error
            .as_deref(),
        Some("collector failed")
    );
    service
        .publish_revision(&ExecutionProfileRevision {
            profile_id: profile.id,
            revision: 1,
            digest: "archive".into(),
            size_bytes: 7,
            publisher_id: None,
            expires_at: None,
            created_at: Utc::now(),
            uri: "blob://profile".into(),
        })
        .await
        .unwrap();
    expect_profile_event(broker.as_ref(), org_id).await;
    assert_eq!(
        service
            .fetch(profile.id)
            .await
            .unwrap()
            .unwrap()
            .current_revision,
        Some(1)
    );
    service.remove(profile.id, org_id).await.unwrap();
    expect_profile_event(broker.as_ref(), org_id).await;
    std::fs::remove_file(path).unwrap();
}
