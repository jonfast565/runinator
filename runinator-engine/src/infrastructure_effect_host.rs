//! Infrastructure-owned workflow VM effects.
//!
//! Provider workers and this host consume different executor classes. External interactions stay
//! registered by their durable effect receipt; automatic coordination executes here without
//! reconstructing a legacy node run.

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, TimeZone, Utc};
use runinator_broker_core::{Broker, EffectDelivery, EffectResultMessage, WakeMessage};
use runinator_comm::{EffectExecutor, EffectResult, WakeCommand};
use runinator_models::workflow_vm::{WorkflowEffectRequest, WorkflowEffectStatus};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, WorkflowVmStore},
};
use tokio::{sync::Notify, task::JoinSet};
use tracing::{info, warn};

const CONSUMER_ID: &str = "runinator-infrastructure-effects";

pub async fn run_infrastructure_effect_host<T: RuntimeStore + WorkflowVmStore + DefinitionStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    shutdown: Arc<Notify>,
) {
    info!("workflow VM infrastructure effect host started");
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                tasks.shutdown().await;
                return;
            }
            Some(joined) = tasks.join_next(), if !tasks.is_empty() => {
                if let Err(err) = joined {
                    warn!(error = %err, "infrastructure effect task failed");
                }
            }
            received = broker.receive_infrastructure_effect(CONSUMER_ID), if tasks.len() < 64 => {
                match received {
                    Ok(delivery) => {
                        let broker = broker.clone();
                        let db = db.clone();
                        tasks.spawn(async move { handle_delivery(db, broker, delivery).await });
                    }
                    Err(err) => {
                        warn!(error = %err, "failed to receive infrastructure effect");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

/// What the host should do with one infrastructure effect delivery.
enum Outcome {
    /// Publish this result on the effect-result channel now.
    Settle(EffectResult),
    /// Nothing to publish. The durable effect receipt is the registration for an external
    /// interaction or coordinator, and its owning adapter settles that effect id later.
    Registered,
    /// The effect completes at a known future instant. Arm a timer wake carrying the result the
    /// host would have returned, rather than holding a task open for the whole wait: the waker
    /// sleeps on it and hands the result back through the ingress channel once due.
    Timer {
        due_at: DateTime<Utc>,
        result: EffectResult,
    },
}

async fn handle_delivery<T: RuntimeStore + WorkflowVmStore + DefinitionStore>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    delivery: EffectDelivery,
) {
    let acknowledged = match execute(db.as_ref(), &delivery).await {
        Outcome::Settle(result) => match broker
            .publish_effect_result(EffectResultMessage {
                dedupe_key: Some(result.event_id.to_string()),
                result,
                enqueued_at: chrono::Utc::now(),
            })
            .await
        {
            Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
                broker.ack_effect(CONSUMER_ID, delivery.delivery_id).await
            }
            Err(err) => {
                warn!(error = %err, effect_id = %delivery.command.effect_id, "failed to publish infrastructure effect result");
                broker.nack_effect(CONSUMER_ID, delivery.delivery_id).await
            }
        },
        Outcome::Timer { due_at, result } => {
            let wake = WakeCommand::new(due_at, result, delivery.command.trace_id);
            match broker
                .publish_wake(WakeMessage {
                    dedupe_key: Some(wake.dedupe_key()),
                    command: wake,
                    enqueued_at: chrono::Utc::now(),
                })
                .await
            {
                // a duplicate means this attempt's timer is already armed, which is exactly what
                // the dedupe key is for: a redelivered effect must not arm a second wake.
                Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
                    info!(
                        effect_id = %delivery.command.effect_id,
                        due_at = %due_at,
                        "armed a timer wake for an infrastructure effect",
                    );
                    broker.ack_effect(CONSUMER_ID, delivery.delivery_id).await
                }
                // nacking keeps the effect delivery as the durable record of the pending timer, so
                // a broker that refused the wake redelivers the effect instead of dropping it.
                Err(err) => {
                    warn!(error = %err, effect_id = %delivery.command.effect_id, "failed to arm the timer wake");
                    broker.nack_effect(CONSUMER_ID, delivery.delivery_id).await
                }
            }
        }
        Outcome::Registered => broker.ack_effect(CONSUMER_ID, delivery.delivery_id).await,
    };
    if let Err(err) = acknowledged {
        warn!(error = %err, effect_id = %delivery.command.effect_id, "failed to settle infrastructure effect delivery");
    }
}

/// Build the outcome for an effect that completes at `due_at` with a fixed result.
///
/// An already-due instant settles inline: routing it through the wake channel would add a hop for
/// no wait. Otherwise the result's timestamp is stamped at `due_at` rather than at relay time, so
/// a late or requeued wake records the settlement at the instant the effect actually completed.
fn at_instant(
    command: &runinator_comm::EffectCommand,
    due_at: DateTime<Utc>,
    status: WorkflowEffectStatus,
    output: Option<runinator_models::value::Value>,
    message: Option<String>,
) -> Outcome {
    let mut result = EffectResult::status(command, status, output, message);
    if due_at <= Utc::now() {
        return Outcome::Settle(result);
    }
    result.timestamp = due_at;
    Outcome::Timer { due_at, result }
}

/// `due_at` as unix seconds, saturating at the representable range.
fn instant_from_unix(due_at: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(due_at, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

async fn execute<T: RuntimeStore + WorkflowVmStore + DefinitionStore>(
    db: &T,
    delivery: &EffectDelivery,
) -> Outcome {
    let command = &delivery.command;
    if command.executor != EffectExecutor::Infrastructure {
        return Outcome::Settle(failed(
            command,
            "provider effect reached infrastructure host",
        ));
    }
    if let Err(err) = command.ensure_supported() {
        return Outcome::Settle(failed(command, err.to_string()));
    }

    match &command.request {
        WorkflowEffectRequest::Timer { due_at } => at_instant(
            command,
            instant_from_unix(*due_at),
            WorkflowEffectStatus::Succeeded,
            None,
            None,
        ),
        // measured from the delivery's enqueue time, not from now, so a redelivered or delayed
        // effect never restarts its own clock.
        WorkflowEffectRequest::TimerDelay { seconds } => at_instant(
            command,
            delivery.enqueued_at + chrono::Duration::seconds(*seconds),
            WorkflowEffectStatus::Succeeded,
            None,
            None,
        ),
        WorkflowEffectRequest::ChildRun {
            workflow_id,
            workflow_name,
            workflow_revision,
            workflow_revision_digest,
            input,
            wait,
            run_name,
            ..
        } => Outcome::Settle(
            match execute_child_run(
                db,
                command,
                *workflow_id,
                workflow_name.as_deref(),
                *workflow_revision,
                workflow_revision_digest.as_deref(),
                input.clone(),
                *wait,
                run_name.clone(),
            )
            .await
            {
                Ok(output) => EffectResult::status(
                    command,
                    WorkflowEffectStatus::Succeeded,
                    Some(output),
                    None,
                ),
                Err(error) => failed(command, error.to_string()),
            },
        ),
        WorkflowEffectRequest::AwaitRun {
            workflow,
            key,
            run_id,
            mode,
        } => Outcome::Settle(
            match execute_await_run(db, command, workflow, key.as_ref(), run_id.as_ref(), mode)
                .await
            {
                Ok(output) => EffectResult::status(
                    command,
                    WorkflowEffectStatus::Succeeded,
                    Some(output),
                    None,
                ),
                Err(error) => failed(command, error.to_string()),
            },
        ),
        WorkflowEffectRequest::Gate {
            kind: runinator_models::orchestration::GateKind::Condition,
            condition,
            poll_interval_seconds,
            deadline_seconds,
            continue_on_timeout,
            ..
        } => Outcome::Settle(
            match execute_condition_gate(
                db,
                command,
                condition,
                *poll_interval_seconds,
                *deadline_seconds,
                *continue_on_timeout,
            )
            .await
            {
                Ok(output) => EffectResult::status(
                    command,
                    WorkflowEffectStatus::Succeeded,
                    Some(output),
                    None,
                ),
                Err(error) => failed(command, error.to_string()),
            },
        ),
        // the approval is normally settled by an operator decision; this arms only its expiry. a
        // decision that lands first settles the effect, and the due wake is then rejected as a
        // stale settle of an already-terminal effect.
        WorkflowEffectRequest::Approval {
            expires_at: Some(expires_at),
            ..
        } => at_instant(
            command,
            instant_from_unix(*expires_at),
            WorkflowEffectStatus::TimedOut,
            None,
            Some("approval expired".into()),
        ),
        // a manual or external gate that nobody opens: this arms only the deadline, on the same
        // terms as the approval expiry above.
        WorkflowEffectRequest::Gate {
            deadline_seconds: Some(deadline),
            continue_on_timeout,
            ..
        } => at_instant(
            command,
            delivery.enqueued_at + chrono::Duration::seconds((*deadline).max(0)),
            if *continue_on_timeout {
                WorkflowEffectStatus::Succeeded
            } else {
                WorkflowEffectStatus::TimedOut
            },
            Some(runinator_models::json!({ "passed": false, "timed_out": true })),
            Some("gate deadline elapsed".into()),
        ),
        WorkflowEffectRequest::Action { .. } => Outcome::Settle(failed(
            command,
            "provider action reached infrastructure host",
        )),
        WorkflowEffectRequest::MutexAcquire { key } => {
            Outcome::Settle(execute_mutex(db, command, key, Duration::from_millis(250), None).await)
        }
        WorkflowEffectRequest::Coordination { kind, input } => {
            execute_coordination(db, delivery, kind, input).await
        }
        _ => Outcome::Registered,
    }
}

async fn execute_mutex<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    key: &str,
    poll_interval: Duration,
    deadline: Option<DateTime<Utc>>,
) -> EffectResult {
    loop {
        if deadline.is_some_and(|deadline| Utc::now() >= deadline) {
            return EffectResult::status(
                command,
                WorkflowEffectStatus::TimedOut,
                None,
                Some(format!("mutex '{key}' was not acquired before its timeout")),
            );
        }
        match db
            .claim_workflow_vm_mutex(
                key.to_string(),
                command.workflow_run_id,
                command.continuation_id,
                chrono::Utc::now().timestamp(),
            )
            .await
        {
            Ok(true) => {
                return EffectResult::status(
                    command,
                    WorkflowEffectStatus::Succeeded,
                    Some(runinator_models::json!({ "key": key, "acquired": true })),
                    None,
                );
            }
            Ok(false) => tokio::time::sleep(poll_interval).await,
            Err(error) => return failed(command, error.to_string()),
        }
    }
}

async fn execute_coordination<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    delivery: &EffectDelivery,
    kind: &str,
    input: &runinator_models::value::Value,
) -> Outcome {
    let command = &delivery.command;
    let output = match kind {
        "audit" => {
            db.record_audit_log(runinator_models::json!({
                "workflow_run_id": command.workflow_run_id,
                "continuation_id": command.continuation_id,
                "effect_id": command.effect_id,
                "payload": input,
            }))
            .await
        }
        "checkpoint" => {
            db.create_automation_record(
                "workflow_checkpoint".into(),
                runinator_models::json!({
                    "workflow_run_id": command.workflow_run_id,
                    "continuation_id": command.continuation_id,
                    "effect_id": command.effect_id,
                    "payload": input,
                    "captured_at": chrono::Utc::now().timestamp(),
                }),
            )
            .await
        }
        "mutex" => {
            let key = input
                .get("name")
                .and_then(runinator_models::value::Value::as_str)
                .unwrap_or("default");
            if input
                .get("release")
                .and_then(runinator_models::value::Value::as_bool)
                .unwrap_or(false)
            {
                return Outcome::Settle(
                    match db
                        .release_workflow_vm_mutex(
                            key.to_string(),
                            command.workflow_run_id,
                            command.continuation_id,
                            chrono::Utc::now().timestamp(),
                        )
                        .await
                    {
                        Ok(()) => EffectResult::status(
                            command,
                            WorkflowEffectStatus::Succeeded,
                            Some(runinator_models::json!({ "key": key, "released": true })),
                            None,
                        ),
                        Err(error) => failed(command, error.to_string()),
                    },
                );
            }
            let poll_interval = input
                .get("poll_interval_seconds")
                .and_then(runinator_models::value::Value::as_i64)
                .filter(|seconds| *seconds > 0)
                .map(|seconds| Duration::from_secs(seconds as u64))
                .unwrap_or_else(|| Duration::from_millis(250));
            let deadline = input
                .get("timeout_seconds")
                .and_then(runinator_models::value::Value::as_i64)
                .filter(|seconds| *seconds > 0)
                .map(|seconds| delivery.enqueued_at + chrono::Duration::seconds(seconds));
            return Outcome::Settle(execute_mutex(db, command, key, poll_interval, deadline).await);
        }
        "debounce" => {
            let seconds = input
                .get("delay_seconds")
                .and_then(runinator_models::value::Value::as_i64)
                .unwrap_or(30)
                .max(0);
            return at_instant(
                command,
                delivery.enqueued_at + chrono::Duration::seconds(seconds),
                WorkflowEffectStatus::Succeeded,
                Some(runinator_models::json!({ "elapsed": true, "delay_seconds": seconds })),
                None,
            );
        }
        "cooldown" => {
            let name = input
                .get("name")
                .and_then(runinator_models::value::Value::as_str)
                .unwrap_or("default");
            let window = input
                .get("window_seconds")
                .and_then(runinator_models::value::Value::as_i64)
                .unwrap_or(60);
            return Outcome::Settle(
                match db
                    .claim_cooldown(name.to_string(), window, chrono::Utc::now().timestamp())
                    .await
                {
                    Ok(remaining) => EffectResult::status(
                        command,
                        WorkflowEffectStatus::Succeeded,
                        Some(runinator_models::json!({
                            "name": name,
                            "skipped": remaining.is_some(),
                            "remaining_seconds": remaining.unwrap_or(0),
                        })),
                        None,
                    ),
                    Err(error) => failed(command, error.to_string()),
                },
            );
        }
        // These coordination points are deliberately settled by their external producer through
        // the effect settlement endpoint; the receipt is their durable registration.
        "collect" => return Outcome::Registered,
        "barrier" => return Outcome::Settle(execute_barrier(db, command, input).await),
        "throttle" | "circuit_breaker" => {
            return Outcome::Settle(execute_record_coordination(db, command, kind, input).await);
        }
        unsupported => {
            return Outcome::Settle(failed(
                command,
                format!("unsupported infrastructure coordination effect '{unsupported}'"),
            ));
        }
    };
    Outcome::Settle(match output {
        Ok(record) => {
            EffectResult::status(command, WorkflowEffectStatus::Succeeded, Some(record), None)
        }
        Err(error) => failed(command, error.to_string()),
    })
}

async fn execute_barrier<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    input: &runinator_models::value::Value,
) -> EffectResult {
    let name = input
        .get("name")
        .and_then(runinator_models::value::Value::as_str)
        .unwrap_or("default");
    let expected = input
        .get("count")
        .and_then(runinator_models::value::Value::as_i64)
        .unwrap_or(1)
        .max(1) as usize;
    let record_type = "workflow_barrier".to_string();
    let mut registered = false;
    loop {
        let records = match db
            .fetch_automation_records(record_type.clone(), None, None)
            .await
        {
            Ok(records) => records,
            Err(error) => return failed(command, error.to_string()),
        };
        let existing = records.into_iter().find(|record| {
            record
                .get("name")
                .and_then(runinator_models::value::Value::as_str)
                == Some(name)
        });
        let mut arrivals = existing
            .as_ref()
            .and_then(|record| record.get("arrivals"))
            .and_then(runinator_models::value::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let run = runinator_models::value::Value::String(command.workflow_run_id.to_string());
        if !registered && !arrivals.contains(&run) {
            arrivals.push(run);
            let record = runinator_models::json!({
                "name": name,
                "expected_count": expected,
                "arrivals": arrivals,
            });
            let stored = if let Some(id) = existing
                .as_ref()
                .and_then(|record| record.get("id"))
                .and_then(runinator_models::value::Value::as_str)
                .and_then(|id| id.parse().ok())
            {
                db.update_automation_record(record_type.clone(), id, record)
                    .await
            } else {
                db.create_automation_record(record_type.clone(), record)
                    .await
            };
            if let Err(error) = stored {
                return failed(command, error.to_string());
            }
            registered = true;
        }
        if arrivals.len() >= expected {
            return EffectResult::status(
                command,
                WorkflowEffectStatus::Succeeded,
                Some(runinator_models::json!({
                    "name": name,
                    "arrivals": arrivals,
                    "count": arrivals.len(),
                })),
                None,
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn execute_record_coordination<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    kind: &str,
    input: &runinator_models::value::Value,
) -> EffectResult {
    let record_type = format!("workflow_{kind}");
    let name = input
        .get("name")
        .and_then(runinator_models::value::Value::as_str)
        .unwrap_or("default");
    if kind == "circuit_breaker" {
        let records = match db
            .fetch_automation_records(record_type.clone(), None, None)
            .await
        {
            Ok(records) => records,
            Err(error) => return failed(command, error.to_string()),
        };
        let existing = records.into_iter().find(|record| {
            record
                .get("name")
                .and_then(runinator_models::value::Value::as_str)
                == Some(name)
        });
        let now = chrono::Utc::now().timestamp();
        let open = existing.as_ref().is_some_and(|record| {
            record
                .get("circuit_state")
                .and_then(runinator_models::value::Value::as_str)
                == Some("open")
                && now
                    - record
                        .get("last_tripped_at")
                        .and_then(runinator_models::value::Value::as_i64)
                        .unwrap_or(0)
                    < input
                        .get("cooldown_seconds")
                        .and_then(runinator_models::value::Value::as_i64)
                        .unwrap_or(120)
        });
        return EffectResult::status(
            command,
            WorkflowEffectStatus::Succeeded,
            Some(runinator_models::json!({ "name": name, "open": open })),
            None,
        );
    }
    let max = input
        .get("max_per_window")
        .and_then(runinator_models::value::Value::as_i64)
        .unwrap_or(10);
    let window = input
        .get("window_seconds")
        .and_then(runinator_models::value::Value::as_i64)
        .unwrap_or(60);
    loop {
        let records = match db
            .fetch_automation_records(record_type.clone(), None, None)
            .await
        {
            Ok(records) => records,
            Err(error) => return failed(command, error.to_string()),
        };
        let existing = records.into_iter().find(|record| {
            record
                .get("name")
                .and_then(runinator_models::value::Value::as_str)
                == Some(name)
        });
        let now = chrono::Utc::now().timestamp();
        let used = existing
            .as_ref()
            .filter(|record| {
                now - record
                    .get("window_start")
                    .and_then(runinator_models::value::Value::as_i64)
                    .unwrap_or(0)
                    < window
            })
            .and_then(|record| record.get("tokens_used"))
            .and_then(runinator_models::value::Value::as_i64)
            .unwrap_or(0);
        if used < max {
            let record = runinator_models::json!({
                "name": name,
                "tokens_used": used + 1,
                "window_start": existing.as_ref().and_then(|record| record.get("window_start")).and_then(runinator_models::value::Value::as_i64).filter(|start| now - *start < window).unwrap_or(now),
                "max_per_window": max,
                "window_seconds": window,
            });
            let stored = if let Some(id) = existing
                .as_ref()
                .and_then(|record| record.get("id"))
                .and_then(runinator_models::value::Value::as_str)
                .and_then(|id| id.parse().ok())
            {
                db.update_automation_record(record_type.clone(), id, record)
                    .await
            } else {
                db.create_automation_record(record_type.clone(), record)
                    .await
            };
            return match stored {
                Ok(record) => EffectResult::status(
                    command,
                    WorkflowEffectStatus::Succeeded,
                    Some(record),
                    None,
                ),
                Err(error) => failed(command, error.to_string()),
            };
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn execute_condition_gate<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    condition: &runinator_models::workflows::WorkflowCondition,
    poll_interval_seconds: i64,
    deadline_seconds: Option<i64>,
    continue_on_timeout: bool,
) -> Result<runinator_models::value::Value, runinator_models::errors::SendableError> {
    let started = chrono::Utc::now().timestamp();
    loop {
        let continuation = db
            .fetch_workflow_continuation(command.continuation_id)
            .await?
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "gate continuation disappeared",
                )
            })?;
        let context = runinator_models::value::Value::encode(&continuation.locals)?;
        if runinator_workflows::evaluate_workflow_condition(condition, &context)
            .map_err(|error| -> runinator_models::errors::SendableError { Box::new(error) })?
        {
            return Ok(runinator_models::json!({ "passed": true }));
        }
        if deadline_seconds
            .is_some_and(|deadline| chrono::Utc::now().timestamp() - started >= deadline)
        {
            if continue_on_timeout {
                return Ok(runinator_models::json!({ "passed": false, "timed_out": true }));
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "condition gate timed out",
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(poll_interval_seconds.max(1) as u64)).await;
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the effect protocol supplies each child-run identity and execution option independently"
)]
async fn execute_child_run<T: RuntimeStore + WorkflowVmStore + DefinitionStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    workflow_id: Option<uuid::Uuid>,
    workflow_name: Option<&str>,
    workflow_revision: Option<i64>,
    workflow_revision_digest: Option<&str>,
    input: runinator_models::value::Value,
    wait: bool,
    run_name: Option<runinator_models::value::Value>,
) -> Result<runinator_models::value::Value, runinator_models::errors::SendableError> {
    let workflow = if let (Some(id), Some(revision), Some(digest)) =
        (workflow_id, workflow_revision, workflow_revision_digest)
    {
        Some(
            crate::repository::support::fetch_workflow_revision_snapshot(db, id, revision, digest)
                .await?,
        )
    } else if workflow_revision.is_some() || workflow_revision_digest.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "child workflow revision pins require id, revision, and digest",
        )
        .into());
    } else if let Some(id) = workflow_id {
        db.fetch_workflow(id).await?
    } else if let Some(name) = workflow_name {
        db.fetch_workflow_by_name(name.to_string()).await?
    } else {
        None
    }
    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "child workflow not found"))?;
    let workflow_id = workflow.id.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "child workflow has no id")
    })?;
    let name = run_name.and_then(|value| match value {
        runinator_models::value::Value::String(value) => Some(value),
        _ => None,
    });
    let child = crate::repository::create_workflow_vm_run(
        db,
        workflow_id,
        workflow,
        input,
        runinator_models::json!({ "parent_run_id": command.workflow_run_id, "parent_effect_id": command.effect_id }),
        name,
        runinator_models::replicas::WorkflowRunProvenance::default(),
        None,
        None,
    )
    .await?;
    if !wait {
        return Ok(
            runinator_models::json!({ "run_id": child.id, "status": child.status.as_str() }),
        );
    }
    loop {
        let current = db.fetch_workflow_run(child.id).await?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "child workflow run disappeared",
            )
        })?;
        if current.status.is_terminal() {
            return Ok(runinator_models::json!({
                "run_id": current.id,
                "status": current.status.as_str(),
                "message": current.message,
            }));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn execute_await_run<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    command: &runinator_comm::EffectCommand,
    workflow_name: &str,
    key: Option<&runinator_models::value::Value>,
    run_id: Option<&runinator_models::value::Value>,
    mode: &str,
) -> Result<runinator_models::value::Value, runinator_models::errors::SendableError> {
    let workflow = db
        .fetch_workflow_by_name(workflow_name.to_string())
        .await?
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "await workflow not found")
        })?;
    let workflow_id = workflow.id.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "await workflow has no id")
    })?;
    let exact_id = run_id.and_then(value_uuid);
    let correlation = key.and_then(value_string);
    loop {
        let mut matched = db
            .fetch_workflow_runs_for_workflow(workflow_id)
            .await?
            .into_iter()
            .filter(|run| run.id != command.workflow_run_id)
            .filter(|run| exact_id.is_none_or(|id| run.id == id))
            .filter(|run| {
                correlation
                    .as_deref()
                    .is_none_or(|key| run.correlation_key.as_deref() == Some(key))
            })
            .collect::<Vec<_>>();
        matched.sort_by_key(|run| run.created_at);
        let satisfied = if mode == "any" {
            matched.iter().any(|run| run.status.is_terminal())
        } else {
            !matched.is_empty() && matched.iter().all(|run| run.status.is_terminal())
        };
        if satisfied {
            return Ok(runinator_models::json!({
                "run_ids": matched.iter().map(|run| run.id).collect::<Vec<_>>(),
                "statuses": matched.iter().map(|run| run.status.as_str()).collect::<Vec<_>>(),
            }));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn value_uuid(value: &runinator_models::value::Value) -> Option<uuid::Uuid> {
    value_string(value)?.parse().ok()
}

fn value_string(value: &runinator_models::value::Value) -> Option<String> {
    match value {
        runinator_models::value::Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn failed(command: &runinator_comm::EffectCommand, message: impl Into<String>) -> EffectResult {
    EffectResult::status(
        command,
        WorkflowEffectStatus::Failed,
        None,
        Some(message.into()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use runinator_broker_core::{EffectMessage, in_memory::InMemoryBroker};
    use runinator_comm::EffectCommand;
    use runinator_models::{
        revisions::{RevisionSource, WorkflowRevision},
        semver::SemVer,
        types::RuninatorType,
        workflows::{WorkflowDefinition, WorkflowGraph},
    };
    use runinator_store::{DatabaseImpl, RuntimeStore, roles::DefinitionStore};
    use uuid::Uuid;

    #[tokio::test]
    async fn timer_effect_publishes_a_terminal_result() {
        let path =
            std::env::temp_dir().join(format!("runinator-infra-effect-{}.db", Uuid::now_v7()));
        let db = Arc::new(
            runinator_database::sqlite::SqliteDb::new(
                path.to_str().expect("temporary database path"),
            )
            .await
            .unwrap(),
        );
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
        let shutdown = Arc::new(Notify::new());
        let host = tokio::spawn(run_infrastructure_effect_host(
            db,
            broker.clone(),
            shutdown.clone(),
        ));
        let command = command(WorkflowEffectRequest::Timer {
            due_at: Utc::now().timestamp() - 1,
        });
        broker
            .publish_effect(EffectMessage {
                command: command.clone(),
                dedupe_key: None,
                enqueued_at: Utc::now(),
                expires_at: None,
            })
            .await
            .unwrap();
        let delivery =
            tokio::time::timeout(Duration::from_secs(2), broker.receive_effect_result("test"))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(delivery.result.effect_id, command.effect_id);
        assert!(matches!(
            delivery.result.kind,
            runinator_comm::EffectResultKind::Status {
                status: WorkflowEffectStatus::Succeeded,
                ..
            }
        ));
        shutdown.notify_waiters();
        host.await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_future_timer_arms_a_wake_instead_of_sleeping_in_process() {
        let path =
            std::env::temp_dir().join(format!("runinator-infra-effect-{}.db", Uuid::now_v7()));
        let db = Arc::new(
            runinator_database::sqlite::SqliteDb::new(
                path.to_str().expect("temporary database path"),
            )
            .await
            .unwrap(),
        );
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
        let shutdown = Arc::new(Notify::new());
        let host = tokio::spawn(run_infrastructure_effect_host(
            db,
            broker.clone(),
            shutdown.clone(),
        ));
        // an hour out: holding a task open for this is exactly what the wake channel replaces.
        let due_at = Utc::now() + chrono::Duration::hours(1);
        let command = command(WorkflowEffectRequest::Timer {
            due_at: due_at.timestamp(),
        });
        broker
            .publish_effect(EffectMessage {
                command: command.clone(),
                dedupe_key: None,
                enqueued_at: Utc::now(),
                expires_at: None,
            })
            .await
            .unwrap();

        let delivery = tokio::time::timeout(Duration::from_secs(2), broker.receive_wake("test"))
            .await
            .expect("a future timer should arm a wake")
            .unwrap();
        assert_eq!(delivery.command.effect_id(), command.effect_id);
        assert_eq!(delivery.command.due_at.timestamp(), due_at.timestamp());
        // the result is stamped at the due instant, not at arming time, so a late relay records
        // the settlement when the effect actually completed.
        assert_eq!(
            delivery.command.result.timestamp.timestamp(),
            due_at.timestamp()
        );
        assert!(matches!(
            delivery.command.result.kind,
            runinator_comm::EffectResultKind::Status {
                status: WorkflowEffectStatus::Succeeded,
                ..
            }
        ));

        // and nothing is settled yet: the effect completes only when the wake comes due.
        assert!(
            tokio::time::timeout(
                Duration::from_millis(200),
                broker.receive_effect_result("test")
            )
            .await
            .is_err(),
            "a future timer must not settle at arming time"
        );

        shutdown.notify_waiters();
        host.await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_future_approval_expiry_arms_a_timed_out_wake() {
        let path =
            std::env::temp_dir().join(format!("runinator-infra-effect-{}.db", Uuid::now_v7()));
        let db = Arc::new(
            runinator_database::sqlite::SqliteDb::new(
                path.to_str().expect("temporary database path"),
            )
            .await
            .unwrap(),
        );
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
        let shutdown = Arc::new(Notify::new());
        let host = tokio::spawn(run_infrastructure_effect_host(
            db,
            broker.clone(),
            shutdown.clone(),
        ));
        let expires_at = Utc::now() + chrono::Duration::hours(4);
        let command = command(WorkflowEffectRequest::Approval {
            prompt: Default::default(),
            expires_at: Some(expires_at.timestamp()),
        });
        broker
            .publish_effect(EffectMessage {
                command: command.clone(),
                dedupe_key: None,
                enqueued_at: Utc::now(),
                expires_at: None,
            })
            .await
            .unwrap();

        let delivery = tokio::time::timeout(Duration::from_secs(2), broker.receive_wake("test"))
            .await
            .expect("an approval expiry should arm a wake")
            .unwrap();
        assert_eq!(delivery.command.effect_id(), command.effect_id);
        assert!(matches!(
            delivery.command.result.kind,
            runinator_comm::EffectResultKind::Status {
                status: WorkflowEffectStatus::TimedOut,
                ..
            }
        ));

        shutdown.notify_waiters();
        host.await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn an_expired_mutex_wait_returns_timed_out() {
        let path =
            std::env::temp_dir().join(format!("runinator-infra-effect-{}.db", Uuid::now_v7()));
        let db = runinator_database::sqlite::SqliteDb::new(
            path.to_str().expect("temporary database path"),
        )
        .await
        .unwrap();
        let command = command(WorkflowEffectRequest::MutexAcquire {
            key: "creds-sync".into(),
        });

        let result = execute_mutex(
            &db,
            &command,
            "creds-sync",
            Duration::from_secs(10),
            Some(Utc::now() - chrono::Duration::seconds(1)),
        )
        .await;
        assert!(matches!(
            result.kind,
            runinator_comm::EffectResultKind::Status {
                status: WorkflowEffectStatus::TimedOut,
                ..
            }
        ));
        let _ = std::fs::remove_file(path);
    }

    fn command(request: WorkflowEffectRequest) -> EffectCommand {
        EffectCommand {
            version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
            command_id: Uuid::now_v7(),
            effect_id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            continuation_id: Uuid::now_v7(),
            attempt: 0,
            request,
            executor: EffectExecutor::Infrastructure,
            target: Default::default(),
            trace_id: Uuid::now_v7(),
            trace_context: Default::default(),
            idempotency_key: Uuid::now_v7().to_string(),
            notification_delivery_id: None,
        }
    }

    fn revisioned_workflow(metadata: runinator_models::value::Value) -> WorkflowDefinition {
        WorkflowDefinition {
            id: None,
            name: "child".into(),
            key: None,
            namespace: Some("acme.billing".into()),
            org_id: None,
            version: SemVer::new(1, 0, 0),
            enabled: true,
            input_type: RuninatorType::Any,
            definition: WorkflowGraph::from_value(runinator_models::json!({
                "start": "start",
                "metadata": metadata,
                "nodes": [
                    { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                    { "id": "end", "kind": "end" }
                ]
            }))
            .unwrap(),
            created_at: None,
            updated_at: None,
        }
    }

    async fn capture_revision(
        db: &runinator_database::sqlite::SqliteDb,
        workflow: &WorkflowDefinition,
    ) -> WorkflowRevision {
        let revision = WorkflowRevision {
            id: Uuid::nil(),
            workflow_id: workflow.id.unwrap(),
            revision: 0,
            digest: WorkflowRevision::content_digest(
                workflow.version,
                &workflow.input_type,
                &workflow.definition,
            ),
            version: workflow.version,
            name: workflow.name.clone(),
            input_type: workflow.input_type.clone(),
            definition: workflow.definition.clone(),
            source: RevisionSource::Api,
            actor_id: None,
            actor_kind: "test".into(),
            note: None,
            created_at: None,
        };
        db.insert_workflow_revision(&revision)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn pinned_child_run_uses_the_selected_revision_not_the_current_head() {
        let path =
            std::env::temp_dir().join(format!("runinator-pinned-child-{}.db", Uuid::now_v7()));
        let db = runinator_database::sqlite::SqliteDb::new(path.to_str().unwrap())
            .await
            .unwrap();
        db.run_init_scripts(&Vec::new()).await.unwrap();

        let first = db
            .upsert_workflow(&revisioned_workflow(
                runinator_models::json!({ "generation": 1 }),
            ))
            .await
            .unwrap();
        let first_revision = capture_revision(&db, &first).await;

        let mut second = revisioned_workflow(runinator_models::json!({ "generation": 2 }));
        second.id = first.id;
        let second = db.upsert_workflow(&second).await.unwrap();
        let _second_revision = capture_revision(&db, &second).await;

        let output = execute_child_run(
            &db,
            &command(WorkflowEffectRequest::Timer { due_at: 0 }),
            first.id,
            None,
            Some(first_revision.revision),
            Some(&first_revision.digest),
            runinator_models::value::Value::Null,
            false,
            None,
        )
        .await
        .unwrap();
        let child_id = output["run_id"].as_str().unwrap().parse().unwrap();
        let child = db.fetch_workflow_run(child_id).await.unwrap().unwrap();
        assert_eq!(
            child.workflow_snapshot.unwrap().definition.metadata["generation"],
            runinator_models::value::Value::Number(1.into())
        );

        let _ = std::fs::remove_file(path);
    }
}
