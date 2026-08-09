//! notification policy evaluation and emission.
//!
//! policies are declarative rules mapping a runtime failure condition to a severity and a delivery
//! channel. emission lives here (in the engine) rather than in the reducer because the engine owns
//! the terminal-state transition and the durable side effects that follow it.
//!
//! delivery never speaks a vendor protocol from this process. an in-app policy writes the
//! notifications row directly; every other channel is enqueued on the existing action-dispatch
//! outbox so a worker executes the normal slack/email provider, exactly like any other action.

use std::sync::Arc;

use runinator_comm::ActionCommand;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::{SendableError, error_code_or_unknown};
use runinator_models::notifications::{
    NewNotification, NotificationChannel, NotificationEvent, NotificationPolicy,
};
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowAction, WorkflowRun, WorkflowStatus};
use tokio::sync::Notify;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::events::{AppEvent, AppEventKind, EventSender, emit};
use crate::repository;

/// how often the duration-based scanner sweeps open runs.
const SCAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// upper bound on runs inspected per sweep so a large backlog drains over several ticks.
const SCAN_LIMIT: i64 = 500;
/// timeout for a delivery action; a notify send that hangs must not pin a worker permit.
const DELIVERY_TIMEOUT_SECONDS: i64 = 30;
/// synthetic node id delivery actions carry, so worker logs identify them at a glance.
const DELIVERY_NODE_ID: &str = "__notification__";

/// the facts a fired policy renders its message from.
struct EmissionContext {
    workflow_run_id: Option<Uuid>,
    node_id: Option<String>,
    title: String,
    body: String,
    metadata: Value,
    /// distinguishes one logical occurrence, so re-evaluating the same condition is idempotent.
    occurrence: String,
}

/// evaluate the run-terminal policies for a run that has just settled. a run that ended anywhere
/// other than failed/timed-out is not an alertable condition and returns without querying policies.
pub async fn on_run_terminal<T: DatabaseImpl>(db: &T, events: &EventSender, workflow_run_id: Uuid) {
    let Ok(Some(run)) = db.fetch_workflow_run(workflow_run_id).await else {
        return;
    };
    if !matches!(
        run.status,
        WorkflowStatus::Failed | WorkflowStatus::TimedOut
    ) {
        return;
    }
    let context = run_failed_context(db, &run).await;
    dispatch_event(
        db,
        events,
        NotificationEvent::RunFailed,
        run.workflow_id,
        &context,
    )
    .await;

    // a failed run is also where an exhausted node retry surfaces; report the specific node so an
    // on-call reader sees which step burned its attempts rather than only that the run died.
    if let Some(node_context) = retry_exhausted_context(db, &run).await {
        dispatch_event(
            db,
            events,
            NotificationEvent::NodeRetryExhausted,
            run.workflow_id,
            &node_context,
        )
        .await;
    }
}

/// periodically emit the duration-based events (`run_sla_breached`, `run_parked`), which have no
/// transition to hang off. each policy's threshold is applied to the open runs it covers.
pub async fn run_notification_scanner<T: DatabaseImpl>(
    db: Arc<T>,
    events: EventSender,
    shutdown: Arc<Notify>,
) {
    info!("notification scanner started");
    loop {
        if let Err(err) = scan_once(db.as_ref(), &events).await {
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "notification scanner iteration failed: {}", err
            );
        }
        tokio::select! {
            _ = shutdown.notified() => {
                info!("notification scanner shutting down");
                return;
            }
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
        }
    }
}

/// one sweep: for each duration-based event, load its policies and match them against open runs.
async fn scan_once<T: DatabaseImpl>(db: &T, events: &EventSender) -> Result<(), SendableError> {
    for event in [
        NotificationEvent::RunSlaBreached,
        NotificationEvent::RunParked,
    ] {
        let policies = db.fetch_notification_policies_by_event(event).await?;
        // a threshold-less duration policy can never be evaluated; skip rather than alert on every
        // open run the moment the policy is created.
        let policies: Vec<_> = policies
            .into_iter()
            .filter(|policy| policy.threshold_seconds.unwrap_or(0) > 0)
            .collect();
        if policies.is_empty() {
            continue;
        }
        // the smallest threshold bounds the sweep: nothing younger than it can breach any policy.
        let Some(min_threshold) = policies.iter().filter_map(|p| p.threshold_seconds).min() else {
            continue;
        };
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(min_threshold);
        let runs = db
            .fetch_open_workflow_runs_created_before(cutoff, SCAN_LIMIT)
            .await?;
        for run in runs {
            // `run_parked` is about a run sitting in a waiting state, not merely a long-running one.
            if event == NotificationEvent::RunParked && !is_parked(run.status) {
                continue;
            }
            let age = (chrono::Utc::now() - run.created_at).num_seconds();
            for policy in &policies {
                if !policy_covers(policy, run.workflow_id) {
                    continue;
                }
                let Some(threshold) = policy.threshold_seconds else {
                    continue;
                };
                if age < threshold {
                    continue;
                }
                let context = duration_context(db, &run, event, threshold, age).await;
                fire(db, events, policy, &context).await;
            }
        }
    }
    Ok(())
}

/// a run is parked when it is open but blocked on something external rather than progressing.
fn is_parked(status: WorkflowStatus) -> bool {
    matches!(
        status,
        WorkflowStatus::Waiting
            | WorkflowStatus::ApprovalRequired
            | WorkflowStatus::InputRequired
            | WorkflowStatus::Blocked
    )
}

/// a policy applies to a workflow when it is global or names that workflow.
fn policy_covers(policy: &NotificationPolicy, workflow_id: Uuid) -> bool {
    policy.workflow_id.is_none() || policy.workflow_id == Some(workflow_id)
}

/// load the policies for one transition-based event and fire each.
async fn dispatch_event<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    event: NotificationEvent,
    workflow_id: Uuid,
    context: &EmissionContext,
) {
    let policies = match db
        .fetch_matching_notification_policies(event, workflow_id)
        .await
    {
        Ok(policies) => policies,
        Err(err) => {
            warn!(
                workflow_id = %workflow_id,
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to load notification policies: {}",
                err
            );
            return;
        }
    };
    for policy in &policies {
        fire(db, events, policy, context).await;
    }
}

/// persist the notification for a fired policy and, for external channels, enqueue its delivery.
///
/// best-effort by design: a failure to alert must never fail the run that triggered it, so errors
/// are logged and swallowed rather than propagated back into the drive path.
async fn fire<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    policy: &NotificationPolicy,
    context: &EmissionContext,
) {
    let notification = NewNotification {
        workflow_run_id: context.workflow_run_id,
        workflow_node_id: context.node_id.clone(),
        channel: policy.channel.as_str().to_string(),
        severity: policy.severity.as_str().to_string(),
        title: context.title.clone(),
        body: Some(context.body.clone()),
        target: policy.target.clone(),
        metadata: context.metadata.clone(),
        dedupe_key: Some(format!("{}:{}", policy.id, context.occurrence)),
    };
    let created = match db.create_notification_if_absent(&notification).await {
        Ok(Some(created)) => created,
        // already emitted for this policy/occurrence by this or another replica.
        Ok(None) => return,
        Err(err) => {
            warn!(
                policy = %policy.id,
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to persist notification: {}",
                err
            );
            return;
        }
    };

    let org_id = match context.workflow_run_id {
        Some(run_id) => repository::org_id_for_workflow_run(db, run_id).await,
        None => None,
    };
    emit(
        events,
        AppEvent::new(
            org_id,
            AppEventKind::NotificationCreated {
                notification_id: created.id,
            },
        ),
    );

    if policy.channel == NotificationChannel::InApp {
        return;
    }
    if let Err(err) = enqueue_delivery(db, policy, &created.id, context).await {
        warn!(
            policy = %policy.id,
            notification = %created.id,
            error_code = error_code_or_unknown(err.as_ref()),
            "failed to enqueue notification delivery: {}",
            err
        );
    } else {
        events.nudge_action_dispatch_publisher();
    }
}

/// hand an external-channel notification to the action outbox so a worker delivers it through the
/// normal provider path.
async fn enqueue_delivery<T: DatabaseImpl>(
    db: &T,
    policy: &NotificationPolicy,
    notification_id: &Uuid,
    context: &EmissionContext,
) -> Result<(), SendableError> {
    let Some((provider, function)) = policy.channel.provider() else {
        return Err(crate::errors::NOTIFY_UNROUTABLE_CHANNEL.error(policy.channel.as_str()));
    };
    let Some(target) = policy.target.clone().filter(|t| !t.trim().is_empty()) else {
        return Err(crate::errors::NOTIFY_MISSING_TARGET.error(policy.id));
    };

    let delivery = db
        .create_notification_delivery(
            *notification_id,
            Some(policy.id),
            policy.channel,
            Some(target.clone()),
        )
        .await?;

    let configuration = delivery_configuration(policy, &target, context);
    let command = ActionCommand {
        command_id: Uuid::now_v7(),
        // deliveries are not node work: the run id correlates the alert with its cause, and the
        // node run id is a fresh identifier the result path routes by delivery id instead.
        workflow_run_id: context.workflow_run_id.unwrap_or_else(Uuid::nil),
        workflow_node_run_id: Uuid::now_v7(),
        node_id: DELIVERY_NODE_ID.to_string(),
        action: WorkflowAction {
            provider: provider.to_string(),
            function: function.to_string(),
            timeout_seconds: DELIVERY_TIMEOUT_SECONDS,
            configuration,
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: Default::default(),
            idempotency_key: None,
        },
        attempt: 0,
        parameters: Value::Null,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        notification_delivery_id: Some(delivery.id),
        // the delivery row is already the dedupe record for an alert, and its id keys the outbox
        // entry, so a delivery needs no second idempotency reservation.
        idempotency_key: None,
    };
    db.enqueue_action_dispatch(format!("notification:{}", delivery.id), command)
        .await?;
    Ok(())
}

/// build the provider configuration for a delivery. the policy's own configuration is applied last
/// so an operator can override any generated field (notably the credential reference).
fn delivery_configuration(
    policy: &NotificationPolicy,
    target: &str,
    context: &EmissionContext,
) -> runinator_models::workflows::WorkflowObject {
    let mut configuration = match policy.channel {
        NotificationChannel::Slack => runinator_models::json!({
            // resolved late by the worker from the settings store.
            "token": "secret://slack/bot_token",
            "channel": target,
            "text": format!("*{}*\n{}", context.title, context.body),
        }),
        NotificationChannel::Email => runinator_models::json!({
            "to": target,
            "subject": context.title,
            "body": context.body,
        }),
        NotificationChannel::InApp => runinator_models::json!({}),
    };
    if let (Some(base), Some(overrides)) = (
        configuration.as_object_mut(),
        policy.configuration.as_object(),
    ) {
        for (key, value) in overrides {
            base.insert(key.clone(), value.clone());
        }
    }
    runinator_models::workflows::WorkflowObject::from_value(configuration).unwrap_or_default()
}

/// render the message for a failed run.
async fn run_failed_context<T: DatabaseImpl>(db: &T, run: &WorkflowRun) -> EmissionContext {
    let workflow_name = workflow_name(db, run).await;
    let reason = run
        .message
        .clone()
        .unwrap_or_else(|| "no failure message recorded".to_string());
    EmissionContext {
        workflow_run_id: Some(run.id),
        node_id: run.active_node_id.clone(),
        title: format!("{} {}", workflow_name, run.status.as_str()),
        body: format!(
            "Run {} ended {}{}.\n{}",
            run.id,
            run.status.as_str(),
            run.active_node_id
                .as_ref()
                .map(|node| format!(" at node '{node}'"))
                .unwrap_or_default(),
            reason
        ),
        metadata: runinator_models::json!({
            "event": NotificationEvent::RunFailed.as_str(),
            "workflow_id": run.workflow_id,
            "workflow_run_id": run.id,
            "status": run.status.as_str(),
        }),
        // one occurrence per run terminal: a run settles once, so the run id is the whole identity.
        occurrence: format!("run_failed:{}", run.id),
    }
}

/// render the message for the node in a failed run that used up its retry budget, if there is one.
async fn retry_exhausted_context<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Option<EmissionContext> {
    let node_runs = db.fetch_workflow_node_runs(run.id).await.ok()?;
    // attempt is zero-based, so a retried node is any failed node past its first attempt.
    let exhausted = node_runs
        .iter()
        .filter(|node_run| {
            matches!(
                node_run.status,
                WorkflowStatus::Failed | WorkflowStatus::TimedOut
            ) && node_run.attempt > 0
        })
        .max_by_key(|node_run| node_run.attempt)?;
    let workflow_name = workflow_name(db, run).await;
    Some(EmissionContext {
        workflow_run_id: Some(run.id),
        node_id: Some(exhausted.node_id.clone()),
        title: format!(
            "{} exhausted retries on '{}'",
            workflow_name, exhausted.node_id
        ),
        body: format!(
            "Node '{}' in run {} failed after {} attempt(s).\n{}",
            exhausted.node_id,
            run.id,
            exhausted.attempt + 1,
            exhausted
                .message
                .clone()
                .unwrap_or_else(|| "no failure message recorded".to_string())
        ),
        metadata: runinator_models::json!({
            "event": NotificationEvent::NodeRetryExhausted.as_str(),
            "workflow_id": run.workflow_id,
            "workflow_run_id": run.id,
            "node_id": exhausted.node_id,
            "attempts": exhausted.attempt + 1,
        }),
        occurrence: format!("retry_exhausted:{}:{}", run.id, exhausted.node_id),
    })
}

/// render the message for a duration breach.
async fn duration_context<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
    event: NotificationEvent,
    threshold_seconds: i64,
    age_seconds: i64,
) -> EmissionContext {
    let workflow_name = workflow_name(db, run).await;
    let (title, verb) = match event {
        NotificationEvent::RunParked => (
            format!("{workflow_name} parked over threshold"),
            "has been parked",
        ),
        _ => (format!("{workflow_name} breached SLA"), "has been open"),
    };
    EmissionContext {
        workflow_run_id: Some(run.id),
        node_id: run.active_node_id.clone(),
        title,
        body: format!(
            "Run {} {} for {} (threshold {}), currently {}{}.",
            run.id,
            verb,
            humanize_seconds(age_seconds),
            humanize_seconds(threshold_seconds),
            run.status.as_str(),
            run.active_node_id
                .as_ref()
                .map(|node| format!(" at node '{node}'"))
                .unwrap_or_default(),
        ),
        metadata: runinator_models::json!({
            "event": event.as_str(),
            "workflow_id": run.workflow_id,
            "workflow_run_id": run.id,
            "status": run.status.as_str(),
            "threshold_seconds": threshold_seconds,
            "age_seconds": age_seconds,
        }),
        // bucket by threshold so a policy alerts once per run, but re-alerts if an operator raises
        // the threshold and the run breaches the new one too.
        occurrence: format!("{}:{}:{}", event.as_str(), run.id, threshold_seconds),
    }
}

/// prefer the run's own snapshot for the workflow name so an alert names the definition that
/// actually ran, falling back to the live row and finally the id.
async fn workflow_name<T: DatabaseImpl>(db: &T, run: &WorkflowRun) -> String {
    if let Some(snapshot) = run.workflow_snapshot.as_ref()
        && !snapshot.name.trim().is_empty()
    {
        return snapshot.name.clone();
    }
    if let Ok(Some(workflow)) = db.fetch_workflow(run.workflow_id).await {
        return workflow.name;
    }
    run.workflow_id.to_string()
}

/// render a duration the way an on-call reader scans it, not as a raw second count.
fn humanize_seconds(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3600 {
        return format!("{}m", seconds / 60);
    }
    if seconds < 86400 {
        return format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60);
    }
    format!("{}d{}h", seconds / 86400, (seconds % 86400) / 3600)
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
