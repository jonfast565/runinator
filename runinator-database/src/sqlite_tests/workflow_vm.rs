use super::*;
use runinator_comm::EffectCommand;
use runinator_models::interrupt::InterruptSource;
use runinator_models::pipelines::{PIPELINE_GRAPH_VERSION, Pipeline, PipelineGraph};
use runinator_models::runs::{TerminalInteraction, TerminalInteractionState};
use runinator_models::workflow_vm::{
    WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect, WorkflowEffectOutput,
    WorkflowEffectOutputEvent, WorkflowEffectRequest, WorkflowEffectStatus, WorkflowFrame,
    WorkflowInstruction, WorkflowModule, WorkflowVmInterruptHandler,
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
    let config = runinator_models::json!({ "jira": { "base_url": "https://jira.test" } });
    let module = WorkflowModule::new(vec![WorkflowInstruction::Return]);

    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: parameters.clone(),
            config: config.clone(),
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
    assert_eq!(roots[0].locals.get("config"), Some(&config));
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn debug_vm_run_stops_at_its_first_boundary() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-vm-debug-start-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("vm-debug-start"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: Value::Null,
            config: Value::Null,
            state: runinator_models::json!({
                "debug": { "enabled": true, "mode": "breakpoints", "breakpoints": [] }
            }),
            name: None,
            provenance: Default::default(),
            pipeline_run_id: None,
            pipeline_member_attempt_id: None,
            module: WorkflowModule::new(vec![
                WorkflowInstruction::DebugBoundary {
                    label: Some("start".into()),
                },
                WorkflowInstruction::Return,
            ]),
            instruction_pointer: 0,
        })
        .await
        .unwrap();

    let outcomes = runinator_runtime::WorkflowVmHost::new(&db)
        .drive_runnable("test-debug-start".into(), 1)
        .await
        .unwrap();
    assert_eq!(
        outcomes,
        vec![runinator_runtime::WorkflowVmDriveOutcome::Joined {
            workflow_run_id: run.id,
        }]
    );
    let continuation = db
        .fetch_workflow_continuations(run.id)
        .await
        .unwrap()
        .remove(0);
    assert!(continuation.operator_paused);
    assert!(continuation.frames.iter().any(
        |frame| matches!(frame, WorkflowFrame::Debug(debug) if debug.paused && debug.breakpoint.as_deref() == Some("start"))
    ));
}

#[tokio::test]
async fn vm_run_start_materializes_and_fires_each_periodic_timer_once() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-vm-timers-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("vm-timers"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let handlers = vec![
        WorkflowVmInterruptHandler {
            source: InterruptSource::Timer,
            target: 1,
            timer_id: Some("fast".into()),
            interval_seconds: Some(30),
        },
        WorkflowVmInterruptHandler {
            source: InterruptSource::Timer,
            target: 1,
            timer_id: Some("slow".into()),
            interval_seconds: Some(300),
        },
    ];
    let mut module = WorkflowModule::new(vec![WorkflowInstruction::CheckInterrupt {
        handlers: handlers.clone(),
    }]);
    module.interrupt_handlers = handlers;

    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: Value::Null,
            config: Value::Null,
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

    let timers = db
        .fetch_workflow_timer_interrupts_before(Utc::now() + Duration::seconds(301), 10)
        .await
        .unwrap();
    let run_timers: Vec<_> = timers
        .into_iter()
        .filter(|timer| timer.workflow_run_id == run.id)
        .collect();
    assert_eq!(run_timers.len(), 2);
    assert_eq!(
        run_timers
            .iter()
            .map(|timer| (timer.timer_id.as_str(), timer.interval_seconds))
            .collect::<Vec<_>>(),
        vec![("fast", 30), ("slow", 300)]
    );

    let fast = run_timers
        .into_iter()
        .find(|timer| timer.timer_id == "fast")
        .unwrap();
    assert!(
        db.fire_workflow_timer_interrupt(fast.clone(), fast.due_at + Duration::seconds(1))
            .await
            .unwrap()
    );
    assert!(
        !db.fire_workflow_timer_interrupt(fast, Utc::now())
            .await
            .unwrap()
    );

    let root = db
        .fetch_workflow_continuations(run.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(
        root.pending_interrupt
            .as_ref()
            .and_then(|pending| pending.payload.get("timer_id"))
            .and_then(Value::as_str),
        Some("fast")
    );
}

#[tokio::test]
async fn terminal_vm_run_surfaces_the_continuation_failure() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-vm-failure-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("vm-failure"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: Value::Null,
            config: Value::Null,
            state: Value::Null,
            name: None,
            provenance: Default::default(),
            pipeline_run_id: None,
            pipeline_member_attempt_id: None,
            module: WorkflowModule::new(vec![
                WorkflowInstruction::EnterNode {
                    node_id: "config".into(),
                },
                WorkflowInstruction::Pop,
            ]),
            instruction_pointer: 0,
        })
        .await
        .unwrap();

    let outcomes = runinator_runtime::WorkflowVmHost::new(&db)
        .drive_runnable("test-vm".into(), 1)
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![runinator_runtime::WorkflowVmDriveOutcome::Failed {
            workflow_run_id: run.id,
            settled_run_id: Some(run.id),
        }]
    );
    let settled = db.fetch_workflow_run(run.id).await.unwrap().unwrap();
    assert_eq!(settled.status, WorkflowStatus::Failed);
    assert_eq!(settled.message.as_deref(), Some("pop needs a stack value"));
    let journal = db.fetch_workflow_journal(run.id).await.unwrap();
    assert!(matches!(
        journal[1].entry,
        runinator_models::workflow_vm::WorkflowJournalEntry::NodeEntered {
            ref node_id,
            ..
        } if node_id == "config"
    ));
    assert!(matches!(
        journal[2].entry,
        runinator_models::workflow_vm::WorkflowJournalEntry::Failed {
            node_id: Some(ref node_id),
            ..
        } if node_id == "config"
    ));
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
            key: None,
            namespace: None,
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
            config: Value::Null,
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
        WorkflowInstruction::EnterNode {
            node_id: "timer".into(),
        },
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
    let running = db.fetch_workflow_run(run.id).await.unwrap().unwrap();
    assert_eq!(running.status, WorkflowStatus::Running);
    assert!(running.started_at.is_some());
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
        node_id: None,
        request: request.clone(),
        status: WorkflowEffectStatus::Requested,
        current_executor_replica_id: None,
        last_executor_replica_id: None,
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
        notification_delivery_id: None,
    };
    let first = db
        .suspend_on_effect(continuation.clone(), effect.clone(), command.clone())
        .await
        .unwrap();
    assert_eq!(
        db.fetch_workflow_run(run.id).await.unwrap().unwrap().status,
        WorkflowStatus::Sleeping
    );
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
    waiting.pending_node_entries.clear();
    waiting.revision += 1;
    assert_eq!(
        db.fetch_workflow_continuations(run.id).await.unwrap(),
        vec![waiting]
    );
    assert_eq!(
        db.fetch_workflow_effects(run.id).await.unwrap(),
        vec![first.clone()]
    );
    let journal = db.fetch_workflow_journal(run.id).await.unwrap();
    assert_eq!(journal.len(), 3);
    assert!(matches!(
        journal[1].entry,
        runinator_models::workflow_vm::WorkflowJournalEntry::NodeEntered {
            ref node_id,
            ..
        } if node_id == "timer"
    ));
    assert!(matches!(
        journal[2].entry,
        runinator_models::workflow_vm::WorkflowJournalEntry::EffectRequested {
            effect_id: recorded_effect_id,
            instruction_pointer: Some(1),
        } if recorded_effect_id == effect_id
    ));
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
        vec![output_event.clone()]
    );
    let required = WorkflowEffectOutputEvent {
        event_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        attempt: 0,
        output: WorkflowEffectOutput::TerminalInteraction {
            interaction: TerminalInteraction {
                sequence: 1,
                request_id: "login".into(),
                state: TerminalInteractionState::InputRequired,
                prompt: Some("Code".into()),
            },
        },
        created_at: Utc::now().timestamp(),
    };
    assert!(
        db.record_workflow_terminal_interaction(required.clone(), Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(
        db.fetch_workflow_effect(effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkflowEffectStatus::InputRequired
    );
    assert_eq!(
        db.fetch_workflow_run(run.id).await.unwrap().unwrap().status,
        WorkflowStatus::InputRequired
    );
    let accepted = WorkflowEffectOutputEvent {
        event_id: Uuid::now_v7(),
        output: WorkflowEffectOutput::TerminalInteraction {
            interaction: TerminalInteraction {
                sequence: 2,
                request_id: "login".into(),
                state: TerminalInteractionState::InputAccepted,
                prompt: None,
            },
        },
        ..required.clone()
    };
    assert!(
        db.record_workflow_terminal_interaction(accepted.clone(), Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(
        db.fetch_workflow_effect(effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkflowEffectStatus::Running
    );
    // This test uses a timer request, so returning its executor state to running restores the
    // request-derived coarse status rather than pretending the workflow itself is active work.
    assert_eq!(
        db.fetch_workflow_run(run.id).await.unwrap().unwrap().status,
        WorkflowStatus::Sleeping
    );
    let stale = WorkflowEffectOutputEvent {
        event_id: Uuid::now_v7(),
        ..required.clone()
    };
    assert!(
        db.record_workflow_terminal_interaction(stale.clone(), Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(
        db.fetch_workflow_effect(effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkflowEffectStatus::Running
    );
    assert_eq!(
        db.fetch_workflow_effect_output(effect_id).await.unwrap(),
        vec![output_event, required, accepted, stale]
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
    assert_eq!(
        db.fetch_workflow_run(run.id).await.unwrap().unwrap().status,
        WorkflowStatus::Running
    );
    assert_eq!(db.fetch_workflow_effects(run.id).await.unwrap().len(), 1);
    // boot, node entry, effect suspension, operator pause, effect settlement, operator resume
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 6);

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
    assert_eq!(db.fetch_workflow_journal(run.id).await.unwrap().len(), 7);

    let _ = fs::remove_file(path);
}
