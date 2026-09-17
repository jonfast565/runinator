use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use runinator_api::{
    AsyncApiClient, OrchestrationListQuery, PipelineIngressRequest, StaticLocator,
};
use runinator_models::json;
use runinator_models::orchestration::OrchestrationStatus;
use runinator_models::pipelines::{PipelineMemberAttemptStatus, PipelineRunDetail};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{WorkflowEffect, WorkflowEffectOutput, WorkflowEffectStatus};
use runinator_models::workflows::{WorkflowRun, WorkflowStatus};
use sqlx::Row;
use tokio::time::sleep;
use uuid::Uuid;

type E2eResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type ApiClient = AsyncApiClient<StaticLocator>;

#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e brokered_result_path_smoke -- --ignored"]
async fn brokered_result_path_smoke() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;

    let ports = Ports::allocate()?;
    let harness = StackHarness::start(&workspace, ports).await?;
    let api = harness.api_client()?;

    // import the workflow through the unified pack source, the same way the real stack ships workflows.
    harness.import_workflows(&workspace.join("runinator-e2e/fixtures"))?;
    let workflow = api
        .fetch_workflow_by_name("Brokered Result Path Smoke")
        .await?;
    let workflow_id = workflow.id.ok_or("imported smoke workflow has no id")?;

    let (run, effects) = run_workflow_by_id(&api, workflow_id, json!({})).await?;
    let action = latest_effect(&api, &effects, "write_logs").await?;
    assert_eq!(action.status, WorkflowEffectStatus::Succeeded);
    assert_eq!(
        action
            .result
            .as_ref()
            .and_then(|value| value.get("success")),
        Some(&Value::Bool(true))
    );

    let log = poll_effect_chunks(&api, action.id).await?;
    assert!(
        log.contains("broker-smoke-start"),
        "missing streamed stdout chunk: {log}"
    );
    assert!(
        log.contains("broker-smoke-end"),
        "missing streamed stdout chunk: {log}"
    );

    assert_effect_output_persisted(&harness.sqlite_path, run.id, action.id).await?;
    Ok(())
}

/// Proves the durable timer path across independently-running engine, waker, and broker processes.
///
/// The wait is armed by the engine, relayed by the waker over `wake → ingress`, then driven by the
/// engine before the run can finish. Unit tests in either runtime cannot cover this process boundary.
#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e wake_ingress_drive_smoke -- --ignored"]
async fn wake_ingress_drive_smoke() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;
    let harness = StackHarness::start(&workspace, Ports::allocate()?).await?;
    let api = harness.api_client()?;
    let workflow_file = harness.run_dir.join("wake-ingress-drive.rrx");
    fs::write(
        &workflow_file,
        r#"language rexrap-1

namespace runinator.tests.e2e {
    workflow "Wake Ingress Drive Smoke" v1 {
        key wake_ingress_drive_smoke

        do {
            @id("wake") wait 1s
        }
    }
}
"#,
    )?;
    harness.import_workflows(&workflow_file)?;
    let workflow = api
        .fetch_workflow_by_name("Wake Ingress Drive Smoke")
        .await?;
    let (run, effects) = run_workflow_by_id(
        &api,
        workflow.id.ok_or("timer smoke workflow has no id")?,
        json!({}),
    )
    .await?;

    assert_eq!(run.status, WorkflowStatus::Succeeded);
    let wait = latest_effect(&api, &effects, "wake").await?;
    assert_eq!(wait.status, WorkflowEffectStatus::Succeeded);
    Ok(())
}

#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e durable_agent_result_outbox_smoke -- --ignored"]
async fn durable_agent_result_outbox_smoke() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;
    let harness = StackHarness::start(&workspace, Ports::allocate()?).await?;
    let api = harness.api_client()?;
    let side_effect = harness.run_dir.join("side-effect.txt");
    let source = format!(
        "workflow \"Durable Agent Result Outbox\" v1 {{\n    node write_once <- console.run(command: \"sleep 3; printf x >> {}\").timeout(15s)\n}}\n",
        side_effect.display()
    );
    let workflow_file = harness.run_dir.join("durable-outbox.rrx");
    fs::write(&workflow_file, source)?;
    harness.import_workflows(&workflow_file)?;
    let workflow = api
        .fetch_workflow_by_name("Durable Agent Result Outbox")
        .await?;
    let run = api
        .create_workflow_run(workflow.id.ok_or("workflow has no id")?, json!({}))
        .await?;

    wait_for_effect_status(&api, run.id, "write_once", WorkflowEffectStatus::Running).await?;
    harness.supervisor_process("stop", "broker")?;
    sleep(Duration::from_secs(5)).await;
    assert_eq!(fs::read_to_string(&side_effect)?, "x");

    harness.supervisor_process("start", "broker")?;
    let (settled, _) = poll_workflow(&api, run.id).await?;
    assert_eq!(settled.status, WorkflowStatus::Succeeded);
    sleep(Duration::from_secs(3)).await;
    assert_eq!(
        fs::read_to_string(&side_effect)?,
        "x",
        "broker redelivery re-executed a non-idempotent side effect"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e advanced_engine_pack_exercises_runtime_and_pipelines -- --ignored"]
async fn advanced_engine_pack_exercises_runtime_and_pipelines() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;
    let harness = StackHarness::start(&workspace, Ports::allocate()?).await?;
    let api = harness.api_client()?;
    harness.import_workflows(&workspace.join("packs/advanced-engine-tests"))?;

    let retry_marker = harness.run_dir.join("advanced-retry.marker");
    let retry = api
        .fetch_workflow_by_name("Advanced Retry Recovery")
        .await?;
    let (retry_run, retry_effects) = run_workflow_by_id(
        &api,
        retry.id.ok_or("advanced retry workflow has no id")?,
        json!({
            "retry_marker": retry_marker.to_string_lossy()
        }),
    )
    .await?;
    assert_eq!(retry_run.status, WorkflowStatus::Succeeded);
    assert!(retry_marker.exists(), "first retry attempt did not run");
    assert!(
        retry_effects
            .iter()
            .any(|effect| effect.node_id.as_deref() == Some("action_1")
                && effect.status == WorkflowEffectStatus::Succeeded),
        "retried action never settled successfully"
    );

    let pipelines = api.fetch_pipelines().await?;
    let mapped = pipelines
        .iter()
        .find(|pipeline| {
            pipeline.artifact_path().qualified() == "runinator.tests.advanced.mapped_fanin"
        })
        .ok_or("advanced mapped fan-in pipeline was not imported")?;
    let mapped_run = api
        .create_pipeline_run(
            mapped.id.ok_or("mapped fan-in pipeline has no id")?,
            json!({ "value": 5, "expected": 25 }),
        )
        .await?;
    let mapped_detail = poll_pipeline(&api, mapped_run.id).await?;
    assert_eq!(mapped_detail.run.status, WorkflowStatus::Succeeded);
    assert_eq!(mapped_detail.attempts.len(), 3);
    let fanin_attempt = mapped_detail
        .attempts
        .iter()
        .find(|attempt| attempt.member_key == "runinator.tests.advanced.pipeline_fanin")
        .ok_or("mapped fan-in verifier did not run")?;
    assert_eq!(fanin_attempt.status, PipelineMemberAttemptStatus::Succeeded);
    assert_eq!(fanin_attempt.parameters.get("left"), Some(&Value::from(10)));
    assert_eq!(
        fanin_attempt.parameters.get("right"),
        Some(&Value::from(15))
    );
    assert_eq!(
        fanin_attempt.parameters.get("expected"),
        Some(&Value::from(25))
    );

    let cleanup_marker = harness.run_dir.join("advanced-pipeline-cleanup.marker");
    let failure_continuation = pipelines
        .iter()
        .find(|pipeline| {
            pipeline.artifact_path().qualified() == "runinator.tests.advanced.failure_continuation"
        })
        .ok_or("advanced failure-continuation pipeline was not imported")?;
    let failure_run = api
        .create_pipeline_run(
            failure_continuation
                .id
                .ok_or("failure-continuation pipeline has no id")?,
            json!({ "cleanup_marker": cleanup_marker.to_string_lossy() }),
        )
        .await?;
    let failure_detail = poll_pipeline(&api, failure_run.id).await?;
    assert_eq!(failure_detail.run.status, WorkflowStatus::Succeeded);
    assert!(failure_detail.attempts.iter().any(|attempt| {
        attempt.member_key == "runinator.tests.advanced.pipeline_failure"
            && attempt.status == PipelineMemberAttemptStatus::Failed
    }));
    assert!(failure_detail.attempts.iter().any(|attempt| {
        attempt.member_key == "runinator.tests.advanced.pipeline_cleanup"
            && attempt.status == PipelineMemberAttemptStatus::Succeeded
    }));
    assert_eq!(
        fs::read_to_string(cleanup_marker)?,
        "cleanup-after-failure\n"
    );
    Ok(())
}

/// Exercises the user-visible mission lifecycle through pack import, ingress admission, two
/// orchestration epochs, evidence reduction, and the mission-prefix list query.
#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e mission_orchestration_transitions_smoke -- --ignored"]
async fn mission_orchestration_transitions_smoke() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;
    let harness = StackHarness::start(&workspace, Ports::allocate()?).await?;
    let api = harness.api_client()?;
    let source = harness.run_dir.join("mission-transitions.rrx");
    fs::write(
        &source,
        r#"language rexrap-1

namespace runinator.tests.e2e {
workflow "Mission Phase One" v1 {
    params { request: any mission: any orchestration: any }
    key mission_phase_one
    do {
        compute {
            return {
                resources_patch: { phase_one: true },
                evidence: { phase: "one" },
                next_member: "runinator.tests.e2e.mission_phase_two"
            }
        }
    }
}

workflow "Mission Phase Two" v1 {
    params { request: any mission: any orchestration: any }
    key mission_phase_two
    do {
        compute {
            return {
                resources_patch: { phase_two: true },
                evidence: { phase: "two" }
            }
        }
    }
}
}

pipeline "Mission Transition Smoke" {
    key mission_transition_smoke
    namespace runinator.tests.e2e
    ingress scope "mission.e2e" { on "start" when unbound -> start }
    orchestration {
        entry "runinator.tests.e2e.mission_phase_one"
        max_epochs 3
        phase "runinator.tests.e2e.mission_phase_one" {
            resources_patch from "/resources_patch"
            evidence from "/evidence"
            next_member from "/next_member"
        }
        phase "runinator.tests.e2e.mission_phase_two" {
            resources_patch from "/resources_patch"
            evidence from "/evidence"
        }
    }
    workflow "runinator.tests.e2e.mission_phase_one"
    workflow "runinator.tests.e2e.mission_phase_two"
}
"#,
    )?;
    harness.import_workflows(&source)?;
    let pipeline = api
        .fetch_pipelines()
        .await?
        .into_iter()
        .find(|pipeline| {
            pipeline.artifact_path().qualified() == "runinator.tests.e2e.mission_transition_smoke"
        })
        .ok_or("mission transition pipeline was not imported")?;
    let correlation = format!("mission-e2e-{}", unique_suffix());
    let admitted = api
        .ingress_pipeline(
            pipeline.id.ok_or("mission transition pipeline has no id")?,
            &PipelineIngressRequest {
                source: "runinator.e2e".into(),
                event_id: Uuid::now_v7().to_string(),
                event_type: "start".into(),
                correlation_key: correlation,
                payload: json!({
                    "request": { "goal": "exercise mission transitions" },
                    "mission": { "kind": "e2e" }
                }),
                provenance: json!({ "origin": "runinator-e2e" }),
            },
        )
        .await?;
    let binding_id = admitted
        .orchestration_binding_id
        .ok_or("mission ingress did not create an orchestration binding")?
        .parse::<Uuid>()?;

    let binding = poll_orchestration(&api, binding_id).await?;
    assert_eq!(binding.status, OrchestrationStatus::Completed);
    assert_eq!(binding.current_epoch, 2);
    assert_eq!(api.fetch_orchestration_epochs(binding_id).await?.len(), 2);
    assert_eq!(api.fetch_orchestration_evidence(binding_id).await?.len(), 2);
    let missions = api
        .fetch_orchestrations_filtered(OrchestrationListQuery {
            limit: Some(1),
            scope_prefix: Some("mission."),
            ..Default::default()
        })
        .await?;
    assert_eq!(missions.first().map(|mission| mission.id), Some(binding_id));
    Ok(())
}

async fn poll_orchestration(
    api: &ApiClient,
    binding_id: Uuid,
) -> E2eResult<runinator_models::orchestration::OrchestrationBinding> {
    for _ in 0..90 {
        let binding = api.fetch_orchestration(binding_id).await?;
        if binding.status.is_terminal() {
            return Ok(binding);
        }
        sleep(Duration::from_secs(1)).await;
    }
    Err(format!("orchestration {binding_id} did not finish in time").into())
}

/// Wait until the effect compiled from `node_id` reaches `expected`.
///
/// The node is found through the run's frozen module source map, which is how a graph node id maps
/// to execution history now that node runs are gone.
async fn wait_for_effect_status(
    api: &ApiClient,
    run_id: Uuid,
    node_id: &str,
    expected: WorkflowEffectStatus,
) -> E2eResult<()> {
    for _ in 0..60 {
        let effects = api.fetch_workflow_effects(run_id).await?;
        if let Ok(effect) = latest_effect(api, &effects, node_id).await
            && effect.status == expected
        {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
    }
    Err(format!("node {node_id} did not reach {expected:?}").into())
}

async fn run_workflow_by_id(
    api: &ApiClient,
    workflow_id: Uuid,
    parameters: Value,
) -> E2eResult<(WorkflowRun, Vec<WorkflowEffect>)> {
    let run = api.create_workflow_run(workflow_id, parameters).await?;
    poll_workflow(api, run.id).await
}

async fn poll_workflow(
    api: &ApiClient,
    workflow_run_id: Uuid,
) -> E2eResult<(WorkflowRun, Vec<WorkflowEffect>)> {
    for _ in 0..60 {
        let run = api.fetch_workflow_run(workflow_run_id).await?;
        if run.status.is_terminal() {
            if run.status == WorkflowStatus::Succeeded {
                let effects = api.fetch_workflow_effects(workflow_run_id).await?;
                return Ok((run, effects));
            }
            return Err(format!(
                "workflow run {workflow_run_id} finished with status {}",
                run.status.as_str()
            )
            .into());
        }
        sleep(Duration::from_secs(2)).await;
    }
    Err(format!("workflow run {workflow_run_id} did not finish in time").into())
}

async fn poll_pipeline(api: &ApiClient, pipeline_run_id: Uuid) -> E2eResult<PipelineRunDetail> {
    for _ in 0..90 {
        let detail = api.fetch_pipeline_run(pipeline_run_id).await?;
        if detail.run.status.is_terminal() {
            return Ok(detail);
        }
        sleep(Duration::from_secs(2)).await;
    }
    Err(format!("pipeline run {pipeline_run_id} did not finish in time").into())
}

/// The newest effect the given graph node produced. `node_id` is the server-side source-map
/// projection the effect list carries.
async fn latest_effect(
    _api: &ApiClient,
    effects: &[WorkflowEffect],
    node_id: &str,
) -> E2eResult<WorkflowEffect> {
    effects
        .iter()
        .filter(|effect| effect.node_id.as_deref() == Some(node_id))
        .max_by_key(|effect| effect.created_at)
        .cloned()
        .ok_or_else(|| format!("missing effect for node {node_id}").into())
}

async fn poll_effect_chunks(api: &ApiClient, effect_id: Uuid) -> E2eResult<String> {
    for _ in 0..30 {
        let events = api.fetch_workflow_effect_output(effect_id).await?;
        let log = events
            .iter()
            .filter_map(|event| match &event.output {
                WorkflowEffectOutput::Chunk { content, .. } => Some(content.as_str()),
                WorkflowEffectOutput::Artifact { .. }
                | WorkflowEffectOutput::Progress { .. }
                | WorkflowEffectOutput::TerminalInteraction { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !log.is_empty() {
            return Ok(log);
        }
        sleep(Duration::from_secs(1)).await;
    }
    Err(format!("workflow effect {effect_id} did not receive chunks").into())
}

/// The streamed output the worker published must be durable, not just observable over HTTP: this
/// is what proves the effect-result consumer wrote it rather than the API synthesising it.
async fn assert_effect_output_persisted(
    sqlite_path: &Path,
    workflow_run_id: Uuid,
    effect_id: Uuid,
) -> E2eResult<()> {
    let url = format!("sqlite://{}", sqlite_path.display());
    let pool = sqlx::SqlitePool::connect(&url).await?;
    let chunks: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM workflow_effect_output_events WHERE effect_id = ?",
    )
    .bind(effect_id)
    .fetch_one(&pool)
    .await?
    .get("count");
    assert!(
        chunks >= 2,
        "expected the effect-result consumer to persist streamed chunks, got {chunks}"
    );

    let settled: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM workflow_journal_entries WHERE workflow_run_id = ? AND entry_json LIKE '%effect_settled%'",
    )
    .bind(workflow_run_id)
    .fetch_one(&pool)
    .await?
    .get("count");
    assert!(
        settled >= 1,
        "expected the run journal to record an effect settlement, got {settled}"
    );
    Ok(())
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn build_service_binaries(workspace: &Path) -> E2eResult<()> {
    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "runinator-supervisor",
            "-p",
            "runinator-broker",
            "-p",
            "runinator-ws",
            "-p",
            "runinator-waker",
            "-p",
            "runinator-worker",
            "-p",
            "runinator-ctl",
            "-p",
            "runinator-bootstrap",
        ])
        .current_dir(workspace)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build for e2e services failed with {status}").into())
    }
}

fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e crate has a workspace parent")
        .to_path_buf()
}

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis();
    format!("{}-{millis}", std::process::id())
}

fn bin_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}

#[path = "stack_smoke/stack_harness.rs"]
mod stack_harness;
use stack_harness::StackHarness;

#[path = "stack_smoke/ports.rs"]
mod ports;
use ports::Ports;
