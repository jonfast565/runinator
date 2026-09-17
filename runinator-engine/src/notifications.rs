//! notification policy evaluation and emission.
//!
//! policies are declarative rules mapping a runtime failure condition to a severity and a delivery
//! channel. emission lives here (in the engine) rather than in the VM because the engine owns
//! the terminal-state transition and the durable side effects that follow it.
//!
//! delivery never speaks a vendor protocol from this process. an in-app policy writes the
//! notifications row directly; every other channel is frozen into the notification-effect outbox
//! and executed by a provider worker without synthesizing workflow node state.

use std::sync::Arc;

use runinator_comm::{EffectCommand, EffectExecutor};
use runinator_models::errors::{SendableError, error_code_or_unknown};
use runinator_models::notifications::{
    NewNotification, NotificationChannel, NotificationEvent, NotificationPolicy,
};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest};
use runinator_models::workflows::{WorkflowRun, WorkflowStatus};
use runinator_models::{settings::SettingKind, settings::SettingRecord};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_secrets::stored_secret::secret_expiry_occurrence;
use runinator_store::{
    RuntimeStore,
    roles::{NewNotificationDelivery, NotificationStore, RunStore, WorkflowVmStore},
};
use tokio::sync::Notify;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::events::{AppEvent, AppEventKind, EventSender, emit};
use crate::repository;
use crate::settings::{ServerSettingsHandle, load_server_settings};

/// the facts a fired policy renders its message from.

/// evaluate the run-terminal policies for a run that has just settled. a run that ended anywhere
/// other than failed/timed-out is not an alertable condition and returns without querying policies.
pub async fn on_run_terminal<T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore>(
    db: &T,
    events: &EventSender,
    workflow_run_id: Uuid,
) {
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
    let delivery_timeout_seconds = load_server_settings(db)
        .await
        .unwrap_or_default()
        .notifications
        .delivery_timeout_seconds as i64;
    let dispatcher = NotificationDispatcher {
        db,
        events,
        delivery_timeout_seconds,
    };
    let context = context_builder.run_failed().await;
    dispatcher
        .dispatch_event(NotificationEvent::RunFailed, run.workflow_id, &context)
        .await;

    // a failed run is also where an exhausted node retry surfaces; report the specific node so an
    // on-call reader sees which step burned its attempts rather than only that the run died.
    let Some(node_context) = context_builder.retry_exhausted().await else {
        return;
    };

    dispatcher
        .dispatch_event(
            NotificationEvent::NodeRetryExhausted,
            run.workflow_id,
            &node_context,
        )
        .await;
}

/// periodically emit scan-based events, which have no transition to hang off. each policy's
/// threshold is applied to the matching run or secret.
pub async fn run_notification_scanner<
    T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore,
>(
    db: Arc<T>,
    events: EventSender,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    info!("notification scanner started");
    loop {
        let policy = settings.current();
        let started = std::time::Instant::now();
        let succeeded = if let Err(err) = scan_once(db.as_ref(), &events, &policy).await {
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
            _ = tokio::time::sleep(std::time::Duration::from_secs(policy.notifications.scan_interval_seconds)) => {}
        }
    }
}

/// one sweep across secret expiry and duration-based run events.
async fn scan_once<T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore>(
    db: &T,
    events: &EventSender,
    settings: &runinator_models::server_settings::ServerSettings,
) -> Result<(), SendableError> {
    scan_secret_expiry(db, events, chrono::Utc::now(), settings).await?;
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
            .fetch_open_workflow_runs_created_before(
                cutoff,
                settings.notifications.scan_limit as i64,
            )
            .await?;
        for run in runs {
            // `run_parked` is about a run sitting in a waiting state, not merely a long-running one.
            if event == NotificationEvent::RunParked && !is_parked(run.status) {
                continue;
            }
            let age = chrono::Utc::now()
                .signed_duration_since(run.created_at)
                .num_seconds();
            let context_builder = EmissionContextBuilder { db, run: &run };
            let dispatcher = NotificationDispatcher {
                db,
                events,
                delivery_timeout_seconds: settings.notifications.delivery_timeout_seconds as i64,
            };
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
async fn scan_secret_expiry<T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore>(
    db: &T,
    events: &EventSender,
    now: chrono::DateTime<chrono::Utc>,
    settings: &runinator_models::server_settings::ServerSettings,
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
    let secrets = db.list_all_settings().await?;
    let dispatcher = NotificationDispatcher {
        db,
        events,
        delivery_timeout_seconds: settings.notifications.delivery_timeout_seconds as i64,
    };
    for record in secrets {
        let Some(expires_at) = secret_expiry(&cipher, &record) else {
            continue;
        };
        let seconds_until_expiry = (expires_at - now).num_seconds();
        for policy in &policies {
            if policy.org_id != record.org_id {
                continue;
            }
            let warning_seconds = policy
                .threshold_seconds
                .unwrap_or(settings.notifications.secret_expiry_warning_seconds as i64);
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
    crate::settings::decode_secret(&plaintext).ok()?.expires_at
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
            "setting_id": record.id,
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
            | WorkflowStatus::Parked
            | WorkflowStatus::Sleeping
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

/// build the provider configuration for a delivery. the policy's own configuration is applied last
/// so an operator can override any generated field (notably the credential reference).
fn delivery_configuration(
    policy: &NotificationPolicy,
    target: &str,
    context: &EmissionContext,
    interaction: Option<&runinator_models::notifications::NotificationInteraction>,
) -> runinator_models::workflows::WorkflowObject {
    let mut configuration = if policy.provider.is_some() {
        runinator_models::json!({
            "target": target,
            "title": context.title,
            "body": context.body,
            "severity": policy.severity.as_str(),
            "workflow_run_id": context.workflow_run_id,
            "interaction": interaction,
        })
    } else {
        match policy.channel {
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
            NotificationChannel::InApp => runinator_models::json!({ "target": target }),
        }
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

mod emission_context;
use emission_context::EmissionContext;

mod notification_dispatcher;
use notification_dispatcher::NotificationDispatcher;

mod emission_context_builder;
use emission_context_builder::EmissionContextBuilder;
