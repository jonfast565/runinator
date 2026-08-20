use super::*;
use runinator_comm::EffectCommand;
use runinator_models::pipelines::{PIPELINE_GRAPH_VERSION, Pipeline, PipelineGraph};
use runinator_models::workflow_vm::{
    WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect, WorkflowEffectOutput,
    WorkflowEffectOutputEvent, WorkflowEffectRequest, WorkflowEffectStatus, WorkflowInstruction,
    WorkflowModule,
};

#[tokio::test]
async fn vm_run_start_freezes_run_module_root_and_journal_together() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-vm-start-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("vm-start"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let parameters = runinator_models::json!({ "customer": "acme" });
    let module = WorkflowModule::new(vec![WorkflowInstruction::Return]);

    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: parameters.clone(),
            state: runinator_models::json!({}),
            name: Some("atomic start".into()),
            provenance: Default::default(),
            pipeline_run_id: None,
            pipeline_member_attempt_id: None,
            module: module.clone(),
            instruction_pointer: 0,
        })
        .await
        .unwrap();

    assert_eq!(
        db.fetch_workflow_module(run.id).await.unwrap(),
        Some(module)
    );
    let roots = db.fetch_workflow_continuations(run.id).await.unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].locals.get("input"), Some(&parameters));
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 1);
    assert!(
        db.claim_ready_nodes(
            "legacy-scheduler".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap()
        .is_empty(),
        "VM starts must not enqueue a legacy ready-node row"
    );
}

#[tokio::test]
async fn terminal_vm_pipeline_members_are_recoverable_until_the_attempt_settles() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-vm-pipeline-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let snapshot = db.upsert_workflow(&workflow("vm-member")).await.unwrap();
    let workflow_id = snapshot.id.unwrap();
    let pipeline = db
        .upsert_pipeline(&Pipeline {
            id: None,
            name: "vm-pipeline".into(),
            description: None,
            org_id: None,
            graph: PipelineGraph {
                version: PIPELINE_GRAPH_VERSION,
                ..Default::default()
            },
            concurrency: Default::default(),
            defaults: Default::default(),
            metadata: Value::Null,
            created_at: None,
            updated_at: None,
        })
        .await
        .unwrap();
    let pipeline_id = pipeline.id.unwrap();
    let pipeline_run = db
        .create_pipeline_run(
            pipeline_id,
            pipeline,
            Value::Null,
            Value::Null,
            Default::default(),
        )
        .await
        .unwrap();
    let attempt = db
        .create_pipeline_member_attempt(
            pipeline_run.id,
            "member".into(),
            workflow_id,
            1,
            Value::Null,
        )
        .await
        .unwrap()
        .unwrap();
    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: Value::Null,
            state: Value::Null,
            name: None,
            provenance: Default::default(),
            pipeline_run_id: Some(pipeline_run.id),
            pipeline_member_attempt_id: Some(attempt.id),
            module: WorkflowModule::new(vec![WorkflowInstruction::Return]),
            instruction_pointer: 0,
        })
        .await
        .unwrap();
    db.settle_workflow_vm_run(run.id, WorkflowStatus::Succeeded, None)
        .await
        .unwrap();

    assert_eq!(
        db.fetch_unsettled_vm_pipeline_members(10).await.unwrap(),
        vec![run.id]
    );
    db.update_pipeline_member_attempt(
        attempt.id,
        runinator_models::pipelines::PipelineMemberAttemptStatus::Succeeded,
        Value::Null,
        None,
    )
    .await
    .unwrap();
    assert!(
        db.fetch_unsettled_vm_pipeline_members(10)
            .await
            .unwrap()
            .is_empty()
    );
}

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
        executor: runinator_comm::EffectExecutor::Infrastructure,
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
    let output_event = WorkflowEffectOutputEvent {
        event_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        attempt: 0,
        output: WorkflowEffectOutput::Chunk {
            stream: "stdout".into(),
            content: "hello".into(),
        },
        created_at: Utc::now().timestamp(),
    };
    assert!(
        db.append_workflow_effect_output(output_event.clone())
            .await
            .unwrap()
    );
    assert!(
        !db.append_workflow_effect_output(output_event.clone())
            .await
            .unwrap()
    );
    assert_eq!(
        db.fetch_workflow_effect_output(effect_id).await.unwrap(),
        vec![output_event]
    );
    assert_eq!(db.pause_workflow_vm_run(run.id).await.unwrap(), 1);
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
        runinator_models::workflow_vm::WorkflowContinuationStatus::Paused
    );
    assert!(resumed.operator_paused);
    assert_eq!(resumed.revision, continuation.revision + 3);
    assert_eq!(db.resume_workflow_vm_run(run.id, false).await.unwrap(), 1);
    assert_eq!(db.fetch_workflow_effects(run.id).await.unwrap().len(), 1);
    // boot, effect suspension, operator pause, effect settlement, operator resume
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 5);

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
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 6);

    let _ = fs::remove_file(path);
}
