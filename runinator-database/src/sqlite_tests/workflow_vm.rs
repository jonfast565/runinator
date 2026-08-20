use super::*;
use runinator_comm::EffectCommand;
use runinator_models::workflow_vm::{
    WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect, WorkflowEffectRequest,
    WorkflowEffectStatus, WorkflowInstruction, WorkflowModule,
};

#[tokio::test]
async fn workflow_vm_effect_suspend_is_atomic_and_deduplicated() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-vm-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("vm"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            Value::Null,
            Value::Null,
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
        },
        WorkflowInstruction::Return,
    ]);
    let root = WorkflowContinuation::start(run.id, module.version);
    db.create_workflow_vm(module.clone(), root.clone())
        .await
        .unwrap();

    let claimed = db
        .claim_runnable_workflow_continuations(
            "scheduler".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    let runinator_runtime::WorkflowVmStep::Yield {
        continuation,
        effect_id,
        sequence,
        request,
    } = runinator_runtime::step_workflow_vm(&module, claimed[0].clone())
    else {
        panic!("expected effect yield");
    };
    let now = Utc::now().timestamp();
    let effect = WorkflowEffect {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        id: effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        sequence,
        attempt: 0,
        request: request.clone(),
        status: WorkflowEffectStatus::Requested,
        result: None,
        message: None,
        created_at: now,
        updated_at: now,
        finished_at: None,
    };
    let command = EffectCommand {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        attempt: 0,
        request,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: effect.idempotency_key(),
    };
    let first = db
        .suspend_on_effect(continuation.clone(), effect.clone(), command.clone())
        .await
        .unwrap();
    let duplicate = db
        .suspend_on_effect(continuation.clone(), effect, command)
        .await
        .unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(
        db.fetch_workflow_module(run.id).await.unwrap(),
        Some(module.clone())
    );
    let mut waiting = continuation.clone();
    waiting.revision += 1;
    assert_eq!(
        db.fetch_workflow_continuations(run.id).await.unwrap(),
        vec![waiting]
    );
    assert_eq!(
        db.fetch_workflow_effects(run.id).await.unwrap(),
        vec![first.clone()]
    );
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 2);
    let dispatches = db
        .claim_pending_workflow_effect_dispatches(
            "effect-publisher".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].effect_id, effect_id);
    db.mark_workflow_effect_dispatch_failed(dispatches[0].id, "broker unavailable".into())
        .await
        .unwrap();
    let retried = db
        .claim_pending_workflow_effect_dispatches(
            "effect-publisher".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].attempts, 1);
    db.mark_workflow_effect_dispatch_published(retried[0].id)
        .await
        .unwrap();
    assert!(
        db.claim_pending_workflow_effect_dispatches(
            "effect-publisher".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
        db.settle_workflow_effect(
            effect_id,
            0,
            WorkflowEffectStatus::Succeeded,
            Some(Value::String("done".into())),
            None,
            Utc::now(),
        )
        .await
        .unwrap()
    );
    assert!(
        !db.settle_workflow_effect(
            effect_id,
            0,
            WorkflowEffectStatus::Succeeded,
            None,
            None,
            Utc::now(),
        )
        .await
        .unwrap()
    );
    let resumed = db
        .fetch_workflow_continuation(continuation.id)
        .await
        .unwrap()
        .expect("resumed continuation");
    assert_eq!(
        resumed.status,
        runinator_models::workflow_vm::WorkflowContinuationStatus::Runnable
    );
    assert_eq!(resumed.revision, continuation.revision + 2);
    assert_eq!(db.fetch_workflow_effects(run.id).await.unwrap().len(), 1);
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 3);

    let mut completed = db
        .fetch_workflow_continuation(continuation.id)
        .await
        .unwrap()
        .expect("continuation after settlement");
    completed.status = runinator_models::workflow_vm::WorkflowContinuationStatus::Succeeded;
    db.commit_workflow_continuation(
        completed,
        runinator_models::workflow_vm::WorkflowJournalEntry::Completed {
            continuation_id: continuation.id,
            value: Value::String("done".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        db.fetch_workflow_continuation(continuation.id)
            .await
            .unwrap()
            .expect("completed continuation")
            .status,
        runinator_models::workflow_vm::WorkflowContinuationStatus::Succeeded
    );
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 4);

    let _ = fs::remove_file(path);
}
