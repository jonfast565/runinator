//! workflow and pipeline definition rows: upsert by name, namespaced resolution, siblings sharing a
//! name, and pipeline create/update/delete.

use super::*;

#[tokio::test]
async fn upsert_workflow_without_id_updates_existing_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-upsert-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let first = db.upsert_workflow(&workflow("pipeline")).await.unwrap();
    let mut updated = workflow("pipeline");
    updated.version = runinator_models::semver::SemVer::new(2, 0, 0);
    updated.definition = WorkflowGraph::from_value(
        runinator_models::json!({ "nodes": [{ "id": "done", "kind": "end" }] }),
    )
    .unwrap();

    let second = db.upsert_workflow(&updated).await.unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(
        second.version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    assert_eq!(second.definition, updated.definition);
    assert_eq!(workflows.len(), 1);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn namespaced_workflow_persists_and_resolves_by_qualified_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-namespace-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // two workflows share the bare name "ticket_work" but live in different namespaces.
    let mut core = workflow("ticket_work");
    core.namespace = Some("core_sdlc".into());
    let mut ops = workflow("ticket_work");
    ops.namespace = Some("ops".into());
    let core = db.upsert_workflow(&core).await.unwrap();
    let ops = db.upsert_workflow(&ops).await.unwrap();
    // distinct namespaces keep them apart rather than colliding on the shared name.
    assert_ne!(core.id, ops.id);
    assert_eq!(core.namespace.as_deref(), Some("core_sdlc"));

    // a qualified subflow target resolves to the matching namespace.
    let resolved = db
        .fetch_workflow_by_name("core_sdlc.ticket_work".into())
        .await
        .unwrap()
        .expect("qualified resolution");
    assert_eq!(resolved.id, core.id);
    assert_eq!(resolved.namespace.as_deref(), Some("core_sdlc"));

    // re-upsert by (namespace, name) identity updates in place, not creating a sibling.
    let again = db.upsert_workflow(&core).await.unwrap();
    assert_eq!(again.id, core.id);
    assert_eq!(db.fetch_workflows().await.unwrap().len(), 2);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn insert_workflow_creates_sibling_row_sharing_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-insert-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let first = db.upsert_workflow(&workflow("pipeline")).await.unwrap();
    let mut copy = workflow("pipeline");
    copy.version = runinator_models::semver::SemVer::new(1, 1, 0);

    let second = db.insert_workflow(&copy).await.unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // a fresh row, not an update of the original.
    assert_ne!(second.id, first.id);
    assert_eq!(second.name, first.name);
    assert_eq!(
        second.version,
        runinator_models::semver::SemVer::new(1, 1, 0)
    );
    assert_eq!(workflows.len(), 2);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn pipeline_round_trip_create_update_delete() {
    use runinator_models::pipelines::{
        Pipeline, PipelineDefaults, PipelineFailurePolicy, PipelineMemberFailureMode,
    };

    let path = std::env::temp_dir().join(format!(
        "runinator-pipeline-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let member = Uuid::new_v4();
    let org = Uuid::new_v4();
    let created = db
        .upsert_pipeline(&Pipeline {
            id: None,
            name: "Release".into(),
            description: Some("ship it".into()),
            org_id: Some(org),
            workflow_ids: vec![member],
            member_failure_modes: [(member, PipelineMemberFailureMode::SilentlyContinue)]
                .into_iter()
                .collect(),
            defaults: PipelineDefaults {
                on_step_failure: PipelineFailurePolicy::Continue,
                links_enabled_by_default: false,
                default_parameters: runinator_models::json!({ "env": "prod" }),
                max_chain_depth: Some(8),
                default_failure_mode: PipelineMemberFailureMode::Stop,
            },
            metadata: Value::Null,
            created_at: None,
            updated_at: None,
        })
        .await
        .unwrap();
    let id = created.id.unwrap();
    assert_eq!(created.org_id, Some(org));
    assert_eq!(created.workflow_ids, vec![member]);
    assert_eq!(
        created.member_failure_modes.get(&member).copied(),
        Some(PipelineMemberFailureMode::SilentlyContinue)
    );
    assert_eq!(
        created.defaults.on_step_failure,
        PipelineFailurePolicy::Continue
    );
    assert!(!created.defaults.links_enabled_by_default);
    assert_eq!(created.defaults.max_chain_depth, Some(8));
    assert_eq!(
        created.defaults.default_failure_mode,
        PipelineMemberFailureMode::Stop
    );

    // update: rename and swap the failure policy; the id and created_at are preserved.
    let mut edit = created.clone();
    edit.name = "Release v2".into();
    edit.defaults.on_step_failure = PipelineFailurePolicy::Halt;
    let updated = db.upsert_pipeline(&edit).await.unwrap();
    assert_eq!(updated.id, Some(id));
    assert_eq!(updated.name, "Release v2");
    assert_eq!(
        updated.defaults.on_step_failure,
        PipelineFailurePolicy::Halt
    );

    let all = db.fetch_pipelines().await.unwrap();
    assert_eq!(all.len(), 1);
    let fetched = db.fetch_pipeline(id).await.unwrap().unwrap();
    assert_eq!(fetched.name, "Release v2");
    // org ownership: the pipeline is discoverable by its org, and reassignment clears it.
    assert_eq!(db.fetch_pipeline_ids_for_org(org).await.unwrap(), vec![id]);
    db.set_pipeline_org(id, None).await.unwrap();
    assert!(
        db.fetch_pipeline(id)
            .await
            .unwrap()
            .unwrap()
            .org_id
            .is_none()
    );
    assert!(db.fetch_pipeline_ids_for_org(org).await.unwrap().is_empty());

    db.delete_pipeline(id).await.unwrap();
    assert!(db.fetch_pipeline(id).await.unwrap().is_none());
    assert!(db.fetch_pipelines().await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}
