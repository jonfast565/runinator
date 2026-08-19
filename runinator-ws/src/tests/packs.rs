//! export and import: upsert ordering, id-present versus id-less overwrite rules, version comparison,
//! sibling duplication, and the managed chained triggers a pipeline import materializes.

use super::*;

#[tokio::test]
async fn export_all_includes_workflows_and_matching_triggers() {
    let (db, path) = test_db().await;
    let saved = save_workflow(&db, &workflow(None, "export-all"))
        .await
        .unwrap();
    let workflow_id = saved.id.unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, workflow_id))
        .await
        .unwrap();

    let bundle = crate::repository::export_workflow_bundle(&db, None)
        .await
        .unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].id, Some(workflow_id));
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].workflow_id, workflow_id);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn export_one_includes_only_that_workflow_and_its_triggers() {
    let (db, path) = test_db().await;
    let first = save_workflow(&db, &workflow(None, "first")).await.unwrap();
    let second = save_workflow(&db, &workflow(None, "second")).await.unwrap();
    let first_id = first.id.unwrap();
    let second_id = second.id.unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, first_id))
        .await
        .unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, second_id))
        .await
        .unwrap();

    let bundle = crate::repository::export_workflow_bundle(&db, Some(second_id))
        .await
        .unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].id, Some(second_id));
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].workflow_id, second_id);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_upserts_workflows_before_triggers() {
    let (db, path) = test_db().await;
    let wf_id = Uuid::now_v7();
    let trig_id = Uuid::now_v7();
    let bundle = WorkflowBundle {
        workflows: vec![workflow(Some(wf_id), "imported")],
        triggers: vec![trigger(Some(trig_id), wf_id)],
    };

    let saved = crate::repository::import_workflow_bundle(&db, bundle)
        .await
        .unwrap();

    assert_eq!(saved.workflows[0].id, Some(wf_id));
    assert_eq!(saved.triggers[0].id, Some(trig_id));
    assert!(db.fetch_workflow(wf_id).await.unwrap().is_some());
    assert_eq!(db.fetch_workflow_triggers(wf_id).await.unwrap().len(), 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_skips_workflow_when_name_already_exists() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let initial_version = initial.workflows[0].version;
    let initial_definition = initial.workflows[0].definition.clone();
    let mut changed = workflow(None, "Core Team SDLC Pipeline");
    changed.version = runinator_models::semver::SemVer::new(2, 0, 0);
    changed.definition = WorkflowGraph::from_value(json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle(&db, second)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // re-importing the same workflow name leaves the existing row untouched.
    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, workflows[0].id);
    assert_eq!(workflows[0].name, "Core Team SDLC Pipeline");
    assert_eq!(workflows[0].version, initial_version);
    assert_eq!(workflows[0].definition, initial_definition);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_overwrite_updates_existing_workflow_in_place() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let existing_id = initial.workflows[0].id;
    assert_ne!(
        initial.workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );

    // an explicit re-apply carries no id and no newer timestamp, but overwrite must still win.
    let mut changed = workflow(None, "Core Team SDLC Pipeline");
    changed.version = runinator_models::semver::SemVer::new(2, 0, 0);
    changed.definition = WorkflowGraph::from_value(json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle_with(&db, second, true)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // the existing row is updated in place: same id, bumped version, no duplicate row. the skip
    // path would have left the stored version unchanged, so version == 2 proves the overwrite.
    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, existing_id);
    assert_eq!(workflows[0].id, existing_id);
    assert_eq!(
        workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_pipeline_creates_managed_chained_triggers_idempotently() {
    use runinator_models::pipelines::{
        PipelineBundle, PipelineLinkSelector, PipelineLinkSpec, PipelineSpec,
    };

    let (db, path) = test_db().await;
    // import the two member workflows first so the pipeline can resolve their names to ids.
    let members = WorkflowBundle {
        workflows: vec![
            workflow(None, "SDLC: Development"),
            workflow(None, "SDLC: Review"),
        ],
        triggers: vec![],
    };
    crate::repository::import_workflow_bundle(&db, members)
        .await
        .unwrap();

    let bundle = PipelineBundle {
        pipelines: vec![PipelineSpec {
            name: "Core SDLC".into(),
            description: Some("test".into()),
            defaults: Default::default(),
            members: vec!["SDLC: Development".into(), "SDLC: Review".into()],
            links: vec![PipelineLinkSpec {
                from: "SDLC: Development".into(),
                to: "SDLC: Review".into(),
                on: PipelineLinkSelector::Complete,
                enabled: true,
                parameters: Default::default(),
            }],
            joins: vec![],
            concurrency: Default::default(),
            triggers: vec![],
        }],
    };

    // first import: creates the pipeline with its first-class graph.
    let imported = crate::repository::import_pipeline_bundle_with(&db, &bundle, None)
        .await
        .unwrap();
    assert_eq!(imported.len(), 1);
    let pipeline_id = imported[0].id.expect("pipeline id");
    assert_eq!(imported[0].graph.members.len(), 2);
    assert_eq!(imported[0].graph.links.len(), 1);

    let dev_id = db
        .fetch_workflow_by_name("SDLC: Development".into())
        .await
        .unwrap()
        .unwrap()
        .id
        .unwrap();
    assert!(db.fetch_workflow_triggers(dev_id).await.unwrap().is_empty());

    // re-import reconciles in place without creating workflow triggers.
    let reimported = crate::repository::import_pipeline_bundle_with(&db, &bundle, None)
        .await
        .unwrap();
    assert_eq!(reimported[0].id, Some(pipeline_id));
    assert_eq!(db.fetch_pipelines().await.unwrap().len(), 1);
    assert!(db.fetch_workflow_triggers(dev_id).await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn manual_pipeline_run_starts_entry_member_chains_and_settles() {
    use runinator_models::pipelines::{
        PipelineBundle, PipelineLinkSelector, PipelineLinkSpec, PipelineSpec,
    };
    use runinator_models::workflows::WorkflowStatus;

    let (db, path) = test_db().await;
    let members = WorkflowBundle {
        workflows: vec![workflow(None, "Build"), workflow(None, "Deploy")],
        triggers: vec![],
    };
    crate::repository::import_workflow_bundle(&db, members)
        .await
        .unwrap();

    // build -> deploy on complete: build is the sole entry member, deploy is downstream.
    let bundle = PipelineBundle {
        pipelines: vec![PipelineSpec {
            name: "Release".into(),
            description: None,
            defaults: Default::default(),
            members: vec!["Build".into(), "Deploy".into()],
            links: vec![PipelineLinkSpec {
                from: "Build".into(),
                to: "Deploy".into(),
                on: PipelineLinkSelector::Complete,
                enabled: true,
                parameters: Default::default(),
            }],
            joins: vec![],
            concurrency: Default::default(),
            triggers: vec![],
        }],
    };
    let imported = crate::repository::import_pipeline_bundle_with(&db, &bundle, None)
        .await
        .unwrap();
    let pipeline_id = imported[0].id.expect("pipeline id");

    // start a manual pipeline run and drive the whole member graph to completion.
    let run = crate::repository::create_manual_pipeline_run(
        &db,
        pipeline_id,
        json!({}),
        None,
        Some("test".into()),
    )
    .await
    .unwrap();
    drain_ready_nodes(&db).await;

    // both members ran, each tagged with the pipeline run, and the chained Deploy inherited the tag.
    let members = db
        .fetch_workflow_runs_for_pipeline_run(run.id)
        .await
        .unwrap();
    assert_eq!(members.len(), 2, "entry member + chained downstream member");
    assert!(members.iter().all(|m| m.pipeline_run_id == Some(run.id)));
    assert!(
        members
            .iter()
            .all(|m| m.status == WorkflowStatus::Succeeded)
    );

    // the pipeline run settles Succeeded once the reachable graph is terminal.
    let settled = db.fetch_pipeline_run(run.id).await.unwrap().unwrap();
    assert_eq!(settled.status, WorkflowStatus::Succeeded);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_upserts_existing_workflow_when_id_is_present() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let existing_id = initial.workflows[0].id;

    // a save from the command center carries the existing id and must overwrite.
    let mut changed = initial.workflows[0].clone();
    changed.version = runinator_models::semver::SemVer::new(2, 0, 0);
    changed.definition = WorkflowGraph::from_value(json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle(&db, second)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, existing_id);
    // an upsert bumps the version to 2; a skip would have left it at 1.
    assert_eq!(
        workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    assert_eq!(
        saved.workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_overwrites_id_less_workflow_when_incoming_is_newer() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "pack")],
        triggers: vec![],
    };
    crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();

    // a pack import carrying a future updated_at is newer than the stored copy.
    let mut newer = workflow(None, "pack");
    newer.version = runinator_models::semver::SemVer::new(5, 0, 0);
    newer.updated_at = chrono::DateTime::from_timestamp(4_102_444_800, 0);
    let saved = crate::repository::import_workflow_bundle(
        &db,
        WorkflowBundle {
            workflows: vec![newer],
            triggers: vec![],
        },
    )
    .await
    .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(
        workflows[0].version,
        runinator_models::semver::SemVer::new(5, 0, 0)
    );
    assert_eq!(
        saved.workflows[0].version,
        runinator_models::semver::SemVer::new(5, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_skips_id_less_workflow_when_incoming_is_older() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "pack")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let initial_version = initial.workflows[0].version;

    // a pack import carrying a past updated_at is older than the stored copy.
    let mut older = workflow(None, "pack");
    older.version = runinator_models::semver::SemVer::new(5, 0, 0);
    older.updated_at = chrono::DateTime::from_timestamp(1, 0);
    crate::repository::import_workflow_bundle(
        &db,
        WorkflowBundle {
            workflows: vec![older],
            triggers: vec![],
        },
    )
    .await
    .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].version, initial_version);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn duplicate_workflow_creates_bumped_sibling() {
    let (db, path) = test_db().await;
    let initial = crate::repository::import_workflow_bundle(
        &db,
        WorkflowBundle {
            workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
            triggers: vec![],
        },
    )
    .await
    .unwrap();
    let original = initial.workflows[0].clone();
    let original_id = original.id.unwrap();

    let copy = crate::repository::duplicate_workflow(
        &db,
        original_id,
        runinator_models::semver::SemVerBump::Minor,
        &runinator_models::revisions::RevisionAuthor::system(
            runinator_models::revisions::RevisionSource::Duplicate,
        ),
    )
    .await
    .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // a new disabled row sharing the name, with the minor version bumped.
    assert_eq!(workflows.len(), 2);
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.name, original.name);
    assert!(!copy.enabled);
    assert_eq!(
        copy.version,
        original
            .version
            .bump(runinator_models::semver::SemVerBump::Minor)
    );
    let _ = std::fs::remove_file(path);
}
