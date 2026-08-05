//! triggers and schedule policy: due-firing idempotence, and the concurrency/catchup/freeze/backfill
//! decisions the claim transaction makes.

use super::*;

#[tokio::test]
async fn due_trigger_firing_is_idempotent_and_advances_next_execution() {
    let path = std::env::temp_dir().join(format!(
        "runinator-trigger-firing-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("trigger-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = Utc::now() - Duration::seconds(60);
    let trigger = db
        .upsert_workflow_trigger(&WorkflowTrigger {
            id: None,
            workflow_id,
            kind: WorkflowTriggerKind::Cron,
            enabled: true,
            configuration: runinator_models::json!({
                "cron": "*/5 * * * * *",
                "parameters": { "source": "cron" }
            }),
            next_execution: Some(due_at),
            blackout_start: None,
            blackout_end: None,
            metadata: runinator_models::json!({ "name": "test-trigger" }),
            created_at: None,
            updated_at: None,
        })
        .await
        .unwrap();

    let first = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    db.update_workflow_trigger_next_execution(trigger.id.unwrap(), Some(due_at))
        .await
        .unwrap();
    let duplicate = db
        .claim_due_workflow_trigger_firings("scheduler-b".into(), Utc::now(), 10)
        .await
        .unwrap();
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first.runs[0].parameters["source"], "cron");
    assert!(duplicate.is_empty());
    assert!(refreshed.next_execution.is_some());

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn chained_trigger_kind_round_trips_and_firing_dedupes() {
    let path = std::env::temp_dir().join(format!(
        "runinator-chained-trigger-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("chained-trigger-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let trigger = db
        .upsert_workflow_trigger(&WorkflowTrigger {
            id: None,
            workflow_id,
            kind: WorkflowTriggerKind::Chained,
            enabled: true,
            configuration: runinator_models::json!({
                "on": "success",
                "target_workflow": "downstream",
                "parameters": {}
            }),
            next_execution: None,
            blackout_start: None,
            blackout_end: None,
            metadata: runinator_models::json!({ "managed_by": "wdl" }),
            created_at: None,
            updated_at: None,
        })
        .await
        .unwrap();

    // the kind must survive the mapper instead of falling back to Manual.
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.kind, WorkflowTriggerKind::Chained);

    // first firing records; a second with the same fire_key is a no-op (exactly-once).
    let source_run = Uuid::now_v7().to_string();
    let first = db
        .try_record_trigger_firing(trigger.id.unwrap(), source_run.clone())
        .await
        .unwrap();
    let second = db
        .try_record_trigger_firing(trigger.id.unwrap(), source_run)
        .await
        .unwrap();
    assert!(first, "first firing should insert");
    assert!(!second, "duplicate firing must be ignored");

    let _ = fs::remove_file(path);
}

/// a workflow whose definition carries a concurrency cap, the way a compiled `concurrency` header
/// does.
fn capped_workflow(name: &str, max: i64, on_conflict: &str) -> WorkflowDefinition {
    let mut definition = workflow(name);
    definition.definition = WorkflowGraph::from_value(runinator_models::json!({
        "nodes": [],
        "metadata": {
            "concurrency": { "max_concurrent_runs": max, "on_conflict": on_conflict }
        }
    }))
    .unwrap();
    definition
}

/// a whole-second timestamp. `next_execution` persists as unix seconds, so a sub-second `now`
/// would never compare equal to what comes back out.
fn seconds_ago(seconds: i64) -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp(Utc::now().timestamp() - seconds, 0).unwrap()
}

fn cron_trigger(
    workflow_id: Uuid,
    due_at: chrono::DateTime<Utc>,
    configuration: Value,
) -> WorkflowTrigger {
    WorkflowTrigger {
        id: None,
        workflow_id,
        kind: WorkflowTriggerKind::Cron,
        enabled: true,
        configuration,
        next_execution: Some(due_at),
        blackout_start: None,
        blackout_end: None,
        metadata: runinator_models::json!({}),
        created_at: None,
        updated_at: None,
    }
}

async fn schedule_test_db(label: &str) -> (SqliteDb, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-{label}-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

#[tokio::test]
async fn concurrency_skip_burns_the_slot_and_advances_the_schedule() {
    let (db, path) = schedule_test_db("concurrency-skip").await;
    let workflow_id = db
        .upsert_workflow(&capped_workflow("skip-test", 1, "skip"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = seconds_ago(60);
    let trigger = db
        .upsert_workflow_trigger(&cron_trigger(
            workflow_id,
            due_at,
            runinator_models::json!({ "cron": "*/5 * * * * *" }),
        ))
        .await
        .unwrap();

    // the first pass fills the single slot; the second is pointed at a *different* due slot, so it
    // is the concurrency cap and not the firing-row dedupe that declines it.
    let first = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    db.update_workflow_trigger_next_execution(trigger.id.unwrap(), Some(seconds_ago(30)))
        .await
        .unwrap();
    let second = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();

    assert_eq!(first.runs.len(), 1);
    assert!(second.runs.is_empty());
    assert_eq!(second.concurrency_skipped, 1);
    // a skipped slot still moves the schedule on; it is dropped, not retried.
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(refreshed.next_execution.unwrap() > due_at);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn concurrency_queue_holds_the_slot_due_instead_of_creating_a_parked_run() {
    let (db, path) = schedule_test_db("concurrency-queue").await;
    let workflow_id = db
        .upsert_workflow(&capped_workflow("queue-test", 1, "queue"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = seconds_ago(60);
    let trigger = db
        .upsert_workflow_trigger(&cron_trigger(
            workflow_id,
            due_at,
            runinator_models::json!({ "cron": "*/5 * * * * *" }),
        ))
        .await
        .unwrap();

    let first = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    let held = seconds_ago(30);
    db.update_workflow_trigger_next_execution(trigger.id.unwrap(), Some(held))
        .await
        .unwrap();
    let blocked = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();

    assert_eq!(first.runs.len(), 1);
    assert!(blocked.runs.is_empty());
    assert_eq!(blocked.concurrency_deferred, 1);
    // the whole point: the slot stays due rather than becoming a run that parks. it is the wake
    // queue this keeps empty.
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.next_execution, Some(held));

    // once the holder finishes, the held slot fires on the next pass.
    db.update_workflow_run_status(
        first.runs[0].id,
        WorkflowStatus::Succeeded,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let released = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert_eq!(released.runs.len(), 1);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn concurrency_cancel_previous_settles_the_running_run_and_reports_it() {
    let (db, path) = schedule_test_db("concurrency-cancel").await;
    let workflow_id = db
        .upsert_workflow(&capped_workflow("cancel-test", 1, "cancel_previous"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = seconds_ago(60);
    let trigger = db
        .upsert_workflow_trigger(&cron_trigger(
            workflow_id,
            due_at,
            runinator_models::json!({ "cron": "*/5 * * * * *" }),
        ))
        .await
        .unwrap();

    let first = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    db.update_workflow_trigger_next_execution(trigger.id.unwrap(), Some(seconds_ago(30)))
        .await
        .unwrap();
    let second = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();

    let superseded = first.runs[0].id;
    assert_eq!(second.runs.len(), 1);
    // the caller needs the ids: the durable state is settled here, but the workers holding that
    // run's in-flight actions still have to be told.
    assert_eq!(second.canceled_run_ids, vec![superseded]);
    assert_eq!(
        db.fetch_workflow_run(superseded)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkflowStatus::Canceled
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn catchup_fire_all_replays_missed_slots_up_to_its_cap() {
    let (db, path) = schedule_test_db("catchup-fire-all").await;
    let workflow_id = db
        .upsert_workflow(&workflow("catchup-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    // five seconds of a per-second cron is five missed slots; the cap takes three of them.
    let due_at = Utc::now() - Duration::seconds(5);
    db.upsert_workflow_trigger(&cron_trigger(
        workflow_id,
        due_at,
        runinator_models::json!({
            "cron": "* * * * * *",
            "catchup": { "policy": "fire_all", "max_slots": 3 }
        }),
    ))
    .await
    .unwrap();

    let batch = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert_eq!(batch.runs.len(), 3);

    // the cap bounds one pass, it does not discard the rest: the re-anchor lands on the first slot
    // the cap did not reach, so the next tick keeps draining the backlog.
    let drained = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert!(!drained.runs.is_empty());

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn catchup_skip_abandons_slots_later_than_its_grace() {
    let (db, path) = schedule_test_db("catchup-skip").await;
    let workflow_id = db
        .upsert_workflow(&workflow("catchup-skip-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = Utc::now() - Duration::seconds(600);
    let trigger = db
        .upsert_workflow_trigger(&cron_trigger(
            workflow_id,
            due_at,
            runinator_models::json!({
                "cron": "*/5 * * * * *",
                "catchup": { "policy": "skip", "grace_seconds": 60 }
            }),
        ))
        .await
        .unwrap();

    let batch = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert!(batch.runs.is_empty());
    assert_eq!(batch.catchup_skipped, 1);
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(refreshed.next_execution.unwrap() > Utc::now() - Duration::seconds(1));

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn an_active_freeze_window_keeps_a_due_trigger_out_of_the_claim() {
    let (db, path) = schedule_test_db("freeze-window").await;
    let workflow_id = db
        .upsert_workflow(&workflow("freeze-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = seconds_ago(60);
    let trigger = db
        .upsert_workflow_trigger(&cron_trigger(
            workflow_id,
            due_at,
            runinator_models::json!({ "cron": "*/5 * * * * *" }),
        ))
        .await
        .unwrap();
    let window = db
        .create_freeze_window(&NewFreezeWindow {
            org_id: None,
            workflow_id: None,
            name: "change freeze".into(),
            reason: None,
            starts_at: Utc::now() - Duration::hours(1),
            ends_at: Utc::now() + Duration::hours(1),
            enabled: true,
        })
        .await
        .unwrap();

    let frozen = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert!(frozen.runs.is_empty());
    // the slot must survive the freeze: advancing past it here would silently lose the run.
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.next_execution, Some(due_at));

    db.delete_freeze_window(window.id).await.unwrap();
    let thawed = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert_eq!(thawed.runs.len(), 1);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn backfill_replays_a_range_without_re_running_slots_the_loop_already_fired() {
    let (db, path) = schedule_test_db("backfill").await;
    let workflow_id = db
        .upsert_workflow(&workflow("backfill-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = Utc::now() - Duration::seconds(5);
    let trigger = db
        .upsert_workflow_trigger(&cron_trigger(
            workflow_id,
            due_at,
            runinator_models::json!({ "cron": "* * * * * *" }),
        ))
        .await
        .unwrap();
    // let the loop fire one slot first, so the backfill has an already-claimed slot to respect.
    let loop_batch = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    assert_eq!(loop_batch.runs.len(), 1);

    let request = BackfillRequest {
        from: Utc::now() - Duration::seconds(10),
        to: Utc::now(),
        limit: None,
        dry_run: false,
    };
    let (response, runs) = db
        .backfill_workflow_trigger(trigger.id.unwrap(), &request)
        .await
        .unwrap();
    assert_eq!(response.already_fired, 1);
    assert_eq!(response.fired as usize, runs.len());
    assert!(response.fired > 0);

    // a dry run reports the same range without creating anything more.
    let dry = BackfillRequest {
        dry_run: true,
        ..request
    };
    let (preview, preview_runs) = db
        .backfill_workflow_trigger(trigger.id.unwrap(), &dry)
        .await
        .unwrap();
    assert!(preview.dry_run);
    assert!(preview_runs.is_empty());

    let _ = fs::remove_file(path);
}
