//! Frozen replay planning, receipt seeding, and acknowledgement enforcement.
use super::*;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{json, workflows::WorkflowDefinition};
use runinator_store::{DatabaseImpl, roles::DefinitionStore};

async fn source() -> (SqliteDb, Uuid) {
    let path = std::env::temp_dir().join(format!("replay-planner-{}.db", Uuid::new_v4()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let snapshot: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "id": null, "name": "replay", "enabled": true,
        "definition": { "start": "start", "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "a" } } },
            { "id": "a", "kind": "action", "action": { "provider": "test", "function": "capture", "configuration": {} }, "transitions": { "next": { "$node": "b" } } },
            { "id": "b", "kind": "action", "action": { "provider": "test", "function": "consume", "configuration": { "prior": { "$ref": { "node": "a", "output": [] } } } }, "transitions": { "next": { "$node": "end" } } },
            { "id": "end", "kind": "end" }
        ] }
    })).unwrap();
    let snapshot = db.upsert_workflow(&snapshot).await.unwrap();
    let module = runinator_workflows::compile_workflow_module(&snapshot).unwrap();
    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            replay_seed: None,
            workflow_id: snapshot.id.unwrap(),
            workflow_snapshot: snapshot,
            parameters: json!({ "value": 5 }),
            config: json!({ "frozen": "original" }),
            state: Value::Null,
            name: None,
            provenance: Default::default(),
            pipeline_run_id: None,
            pipeline_member_attempt_id: None,
            module,
            instruction_pointer: 0,
        })
        .await
        .unwrap();
    runinator_runtime::WorkflowVmHost::new(&db)
        .drive_runnable("replay-test".into(), 1)
        .await
        .unwrap();
    (db, run.id)
}

#[tokio::test]
async fn seeds_successful_prefix_atomically_without_historical_effects() {
    let (db, id) = source().await;
    let effect = db.fetch_workflow_effects(id).await.unwrap().remove(0);
    db.settle_workflow_effect(
        effect.id,
        effect.attempt,
        WorkflowEffectStatus::Succeeded,
        Some(json!({ "receipt": 42 })),
        None,
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    runinator_runtime::WorkflowVmHost::new(&db)
        .drive_runnable("replay-test".into(), 1)
        .await
        .unwrap();
    let before = db.fetch_workflow_journal(id).await.unwrap();
    let plan = replay_plan(&db, id, Some("b".into())).await.unwrap();
    assert_eq!(plan.verdict, ReplayVerdict::Review, "{:?}", plan.reasons);
    assert_eq!(plan.seeded_receipts.len(), 1);
    assert_eq!(plan.seeded_receipts[0].effect_id, effect.id);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(db.fetch_workflow_journal(id).await.unwrap(), before);
    assert!(
        replay_with_options(
            &db,
            id,
            ReplayOptions {
                from_step_id: Some("b".into()),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    let run = replay_with_options(
        &db,
        id,
        ReplayOptions {
            from_step_id: Some("b".into()),
            acknowledge_review: true,
            plan_fingerprint: Some(plan.plan_fingerprint),
        },
    )
    .await
    .unwrap();
    assert!(db.fetch_workflow_effects(run.id).await.unwrap().is_empty());
    let root = db
        .fetch_workflow_continuations(run.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(
        root.locals.get("config"),
        Some(&json!({ "frozen": "original" }))
    );
    let key = format!(
        "{}a",
        runinator_models::workflow_vm::WORKFLOW_NODE_OUTPUT_PREFIX
    );
    assert_eq!(root.locals.get(&key), Some(&json!({ "receipt": 42 })));
    assert_eq!(root.next_effect_sequence, 0);
    assert_eq!(
        db.fetch_workflow_module(run.id).await.unwrap(),
        db.fetch_workflow_module(id).await.unwrap()
    );
}

#[tokio::test]
async fn missing_failed_and_stale_receipts_fail_closed() {
    let (db, id) = source().await;
    assert_eq!(
        replay_plan(&db, id, Some("missing".into()))
            .await
            .unwrap()
            .verdict,
        ReplayVerdict::Blocked
    );
    assert_eq!(
        replay_plan(&db, id, Some("b".into()))
            .await
            .unwrap()
            .verdict,
        ReplayVerdict::Blocked
    );
    let plan = replay_plan(&db, id, None).await.unwrap();
    let effect = db.fetch_workflow_effects(id).await.unwrap().remove(0);
    db.settle_workflow_effect(
        effect.id,
        effect.attempt,
        WorkflowEffectStatus::Failed,
        None,
        Some("failed".into()),
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    assert_eq!(
        replay_plan(&db, id, Some("b".into()))
            .await
            .unwrap()
            .verdict,
        ReplayVerdict::Blocked
    );
    assert!(
        replay_with_options(
            &db,
            id,
            ReplayOptions {
                acknowledge_review: true,
                plan_fingerprint: Some(plan.plan_fingerprint),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn current_definition_changes_do_not_change_a_replay() {
    let (db, id) = source().await;
    let plan = replay_plan(&db, id, None).await.unwrap();
    let mut current = plan.workflow_snapshot.clone().unwrap();
    current.name = "renamed and edited".into();
    current.definition.nodes.clear();
    db.upsert_workflow(&current).await.unwrap();
    let after = replay_plan(&db, id, None).await.unwrap();
    assert_eq!(plan.plan_fingerprint, after.plan_fingerprint);
    assert_eq!(after.workflow_snapshot.unwrap().name, "replay");
}
