//! notification policy evaluation and emission.
//!
//! policies are declarative rules mapping a runtime failure condition to a severity and a delivery
//! channel. emission lives here (in the engine) rather than in the reducer because the engine owns
//! the terminal-state transition and the durable side effects that follow it.
//!
//! delivery never speaks a vendor protocol from this process. an in-app policy writes the
//! notifications row directly; every other channel is frozen into the notification-effect outbox
//! and executed by a provider worker without synthesizing workflow node state.

use std::sync::Arc;

use runinator_comm::{EffectCommand, EffectExecutor};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::{SendableError, error_code_or_unknown};
use runinator_models::notifications::{
    NewNotification, NotificationChannel, NotificationEvent, NotificationPolicy,
};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest};
use runinator_models::workflows::{WorkflowRun, WorkflowStatus};
use runinator_models::{settings::SettingKind, settings::SettingRecord};
use runinator_utilities::secret_cipher::SecretCipher;
use runinator_utilities::stored_secret::secret_expiry_occurrence;
use tokio::sync::Notify;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::events::{AppEvent, AppEventKind, EventSender, emit};
use crate::repository;

/// how often the notification scanner sweeps scan-driven conditions.
const SCAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// upper bound on runs inspected per sweep so a large backlog drains over several ticks.
const SCAN_LIMIT: i64 = 500;
/// warning window used by `secret_expiring` policies that omit `threshold_seconds`.
const DEFAULT_SECRET_EXPIRY_WARNING_SECONDS: i64 = 30 * 24 * 60 * 60;
/// timeout for a delivery action; a notify send that hangs must not pin a worker permit.
const DELIVERY_TIMEOUT_SECONDS: i64 = 30;

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
    let context_builder = EmissionContextBuilder { db, run: &run };
    let dispatcher = NotificationDispatcher { db, events };
    let context = context_builder.run_failed().await;
    dispatcher
        .dispatch_event(NotificationEvent::RunFailed, run.workflow_id, &context)
        .await;

    // a failed run is also where an exhausted node retry surfaces; report the specific node so an
    // on-call reader sees which step burned its attempts rather than only that the run died.
    if let Some(node_context) = context_builder.retry_exhausted().await {
        dispatcher
            .dispatch_event(
                NotificationEvent::NodeRetryExhausted,
                run.workflow_id,
                &node_context,
            )
            .await;
    }
}

/// periodically emit scan-based events, which have no transition to hang off. each policy's
/// threshold is applied to the matching run or secret.
pub async fn run_notification_scanner<T: DatabaseImpl>(
    db: Arc<T>,
    events: EventSender,
    shutdown: Arc<Notify>,
) {
    info!("notification scanner started");
    loop {
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) = scan_once(db.as_ref(), &events).await {
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "notification scanner iteration failed: {}", err
            );
            false
        } else {
            true
        };
        crate::stability::loop_iteration("notification_scanner", succeeded, started.elapsed());
        tokio::select! {
            _ = shutdown.notified() => {
                info!("notification scanner shutting down");
                return;
            }
            _ = tokio::time::sleep(SCAN_INTERVAL) => {}
        }
    }
}

/// one sweep across secret expiry and duration-based run events.
async fn scan_once<T: DatabaseImpl>(db: &T, events: &EventSender) -> Result<(), SendableError> {
    scan_secret_expiry(db, events, chrono::Utc::now()).await?;
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
            let context_builder = EmissionContextBuilder { db, run: &run };
            let dispatcher = NotificationDispatcher { db, events };
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
                let context = context_builder.duration(event, threshold, age).await;
                dispatcher.fire(policy, &context).await;
            }
        }
    }
    Ok(())
}

/// emit each global `secret_expiring` policy for secrets inside its warning window. ciphertext is
/// opened only long enough to read the envelope metadata; notification content never includes the
/// value. a policy/secret/expiry/window tuple is the logical occurrence, so repeated scans dedupe
/// while a rotated secret with a new expiry can warn again.
async fn scan_secret_expiry<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), SendableError> {
    let policies = db
        .fetch_notification_policies_by_event(NotificationEvent::SecretExpiring)
        .await?;
    let policies: Vec<_> = policies
        .into_iter()
        .filter(|policy| policy.workflow_id.is_none())
        .collect();
    if policies.is_empty() {
        return Ok(());
    }

    let cipher = SecretCipher::from_env();
    let secrets = db.list_settings().await?;
    let dispatcher = NotificationDispatcher { db, events };
    for record in secrets {
        let Some(expires_at) = secret_expiry(&cipher, &record) else {
            continue;
        };
        let seconds_until_expiry = (expires_at - now).num_seconds();
        for policy in &policies {
            let warning_seconds = policy
                .threshold_seconds
                .unwrap_or(DEFAULT_SECRET_EXPIRY_WARNING_SECONDS);
            if warning_seconds <= 0 || seconds_until_expiry > warning_seconds {
                continue;
            }
            let context =
                secret_expiry_context(&record, expires_at, warning_seconds, seconds_until_expiry);
            dispatcher.fire(policy, &context).await;
        }
    }
    Ok(())
}

fn secret_expiry(
    cipher: &SecretCipher,
    record: &SettingRecord,
) -> Option<chrono::DateTime<chrono::Utc>> {
    if record.kind != SettingKind::Secret {
        return None;
    }
    let plaintext = cipher.try_decrypt(&record.value)?;
    crate::settings::decode_secret(&plaintext).expires_at
}

fn secret_expiry_context(
    record: &SettingRecord,
    expires_at: chrono::DateTime<chrono::Utc>,
    warning_seconds: i64,
    seconds_until_expiry: i64,
) -> EmissionContext {
    let identity = format!("{}/{}", record.scope, record.name);
    let expired = seconds_until_expiry <= 0;
    let title = if expired {
        format!("Secret {identity} has expired")
    } else {
        format!("Secret {identity} expires soon")
    };
    let timing = if expired {
        format!("expired {} ago", humanize_seconds(-seconds_until_expiry))
    } else {
        format!("expires in {}", humanize_seconds(seconds_until_expiry))
    };
    EmissionContext {
        workflow_run_id: None,
        node_id: None,
        title,
        body: format!(
            "Secret '{identity}' {timing} at {} (warning window {}).",
            expires_at.to_rfc3339(),
            humanize_seconds(warning_seconds),
        ),
        metadata: runinator_models::json!({
            "event": NotificationEvent::SecretExpiring.as_str(),
            "setting_kind": SettingKind::Secret.as_str(),
            "scope": record.scope,
            "name": record.name,
            "expires_at": expires_at,
            "seconds_until_expiry": seconds_until_expiry,
            "warning_seconds": warning_seconds,
        }),
        occurrence: format!(
            "secret_expiring:{}",
            secret_expiry_occurrence(&record.scope, &record.name, expires_at, warning_seconds,)
        ),
    }
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

/// dispatches fired policies to persistence and delivery. `db` and `events` are invariant across
/// every method here — one dispatcher serves an entire scan or terminal-transition callback.
struct NotificationDispatcher<'a, T: DatabaseImpl> {
    db: &'a T,
    events: &'a EventSender,
}

impl<T: DatabaseImpl> NotificationDispatcher<'_, T> {
    /// load the policies for one transition-based event and fire each.
    async fn dispatch_event(
        &self,
        event: NotificationEvent,
        workflow_id: Uuid,
        context: &EmissionContext,
    ) {
        let policies = match self
            .db
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
            self.fire(policy, context).await;
        }
    }

    /// persist the notification for a fired policy and, for external channels, enqueue its
    /// delivery.
    ///
    /// best-effort by design: a failure to alert must never fail the run that triggered it, so
    /// errors are logged and swallowed rather than propagated back into the drive path.
    async fn fire(&self, policy: &NotificationPolicy, context: &EmissionContext) {
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
        let created = match self.db.create_notification_if_absent(&notification).await {
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
            Some(run_id) => repository::org_id_for_workflow_run(self.db, run_id).await,
            None => None,
        };
        emit(
            self.events,
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
        if let Err(err) = self.enqueue_delivery(policy, &created.id, context).await {
            warn!(
                policy = %policy.id,
                notification = %created.id,
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to enqueue notification delivery: {}",
                err
            );
        }
    }

    /// Freeze an external-channel delivery in its own outbox. It reuses the provider-effect
    /// executor and broker transport, but not a workflow receipt, continuation, node run, or the
    /// removed action-dispatch table.
    async fn enqueue_delivery(
        &self,
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

        let delivery_id = Uuid::now_v7();
        let configuration = delivery_configuration(policy, &target, context);
        let command = EffectCommand {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            command_id: Uuid::now_v7(),
            // A notification can be caused by a workflow run, but is never owned by its VM
            // continuation. Global policy deliveries use nil only as a correlation placeholder.
            effect_id: delivery_id,
            workflow_run_id: context.workflow_run_id.unwrap_or_else(Uuid::nil),
            continuation_id: Uuid::nil(),
            attempt: 0,
            request: WorkflowEffectRequest::Action {
                provider: provider.to_string(),
                function: function.to_string(),
                input: configuration.into(),
                timeout_seconds: Some(DELIVERY_TIMEOUT_SECONDS),
                retry: Default::default(),
                tags: Vec::new(),
                required_labels: Default::default(),
                idempotency_key: None,
                function_binding: None,
            },
            executor: EffectExecutor::Provider,
            target: Default::default(),
            trace_id: Uuid::now_v7(),
            trace_context: Default::default(),
            // The delivery id is also the immutable effect id and idempotency key. This allows a
            // redelivery to reuse worker provider protections without becoming workflow work.
            idempotency_key: format!("notification:{delivery_id}"),
            notification_delivery_id: Some(delivery_id),
        };
        self.db
            .create_notification_delivery(
                delivery_id,
                *notification_id,
                Some(policy.id),
                policy.channel,
                Some(target),
                command,
            )
            .await?;
        Ok(())
    }
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

/// builds emission contexts (the facts a fired policy renders its message from) for one run. `db`
/// and `run` are invariant across every method here.
struct EmissionContextBuilder<'a, T: DatabaseImpl> {
    db: &'a T,
    run: &'a WorkflowRun,
}

impl<T: DatabaseImpl> EmissionContextBuilder<'_, T> {
    /// render the message for a failed run.
    async fn run_failed(&self) -> EmissionContext {
        let run = self.run;
        let workflow_name = self.workflow_name().await;
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
            // one occurrence per run terminal: a run settles once, so the run id is the whole
            // identity.
            occurrence: format!("run_failed:{}", run.id),
        }
    }

    /// render the message for the node in a failed run that used up its retry budget, if there is
    /// one.
    async fn retry_exhausted(&self) -> Option<EmissionContext> {
        let run = self.run;
        let module = self.db.fetch_workflow_module(run.id).await.ok()??;
        let exhausted = self
            .db
            .fetch_workflow_effects(run.id)
            .await
            .ok()?
            .into_iter()
            // An action retry is an effect retry. The receipt's request, attempt, and frozen
            // continuation are the durable replacement for a node-run attempt row.
            .filter(|effect| {
                effect.status == runinator_models::workflow_vm::WorkflowEffectStatus::Failed
                    && effect.attempt > 0
                    && matches!(
                        effect.request,
                        runinator_models::workflow_vm::WorkflowEffectRequest::Action { .. }
                    )
            })
            .max_by_key(|effect| effect.attempt)?;
        let continuation = self
            .db
            .fetch_workflow_continuation(exhausted.continuation_id)
            .await
            .ok()??;
        let node_id = module
            .graph_location(continuation.instruction_pointer.saturating_sub(1))?
            .node_id
            .clone();
        let workflow_name = self.workflow_name().await;
        Some(EmissionContext {
            workflow_run_id: Some(run.id),
            node_id: Some(node_id.clone()),
            title: format!("{} exhausted retries on '{}'", workflow_name, node_id),
            body: format!(
                "Node '{}' in run {} failed after {} attempt(s).\n{}",
                node_id,
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
                "node_id": node_id,
                "attempts": exhausted.attempt + 1,
            }),
            occurrence: format!("retry_exhausted:{}:{node_id}", run.id),
        })
    }

    /// render the message for a duration breach.
    async fn duration(
        &self,
        event: NotificationEvent,
        threshold_seconds: i64,
        age_seconds: i64,
    ) -> EmissionContext {
        let run = self.run;
        let workflow_name = self.workflow_name().await;
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
            // bucket by threshold so a policy alerts once per run, but re-alerts if an operator
            // raises the threshold and the run breaches the new one too.
            occurrence: format!("{}:{}:{}", event.as_str(), run.id, threshold_seconds),
        }
    }

    /// prefer the run's own snapshot for the workflow name so an alert names the definition that
    /// actually ran, falling back to the live row and finally the id.
    async fn workflow_name(&self) -> String {
        let run = self.run;
        if let Some(snapshot) = run.workflow_snapshot.as_ref()
            && !snapshot.name.trim().is_empty()
        {
            return snapshot.name.clone();
        }
        if let Ok(Some(workflow)) = self.db.fetch_workflow(run.workflow_id).await {
            return workflow.name;
        }
        run.workflow_id.to_string()
    }
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
