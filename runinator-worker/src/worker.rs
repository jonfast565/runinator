//! VM provider-effect worker and its control/directive side loops.
//!
//! The reducer action protocol deliberately has no consumer here. A worker executes only durable
//! `EffectCommand`s, whose continuation/effect identities are settled by the VM effect host.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::{Broker, BrokerError, ControlDelivery};
use runinator_comm::{ConsumerProfile, ControlKind};
use runinator_models::errors::{SendableError, error_code_or_unknown};
use runinator_plugin::{
    cancel::CancellationToken, load_libraries_from_path, plugin::Plugin, print_libs,
};
use tokio::sync::{Mutex, Notify};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    agent::{
        DirectiveHandler,
        directives::{DirectiveLoopState, run_directive_loop},
        outbox::{ResultOutbox, drain_before_work, drain_forever},
    },
    broker::broker_error,
    events::{WorkerEvent, WorkerEventSink},
    metrics,
    provider_repository::ProviderFactory,
};

const RECEIVE_RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// One provider effect tracked so a targeted or run-wide control cancellation can stop it.
#[derive(Clone)]
pub(crate) struct InFlightAction {
    pub(crate) workflow_run_id: Uuid,
    pub(crate) token: CancellationToken,
    pub(crate) canceled_by_control: Arc<AtomicBool>,
}

/// Everything the VM provider-effect loop needs. The standalone worker and desktop agent share
/// this runtime so provider behavior cannot drift between them.
pub struct WorkerRuntime {
    pub broker: Arc<dyn Broker>,
    pub profile: ConsumerProfile,
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub api_client: AsyncApiClient<StaticLocator>,
    pub providers: ProviderFactory,
    pub max_concurrent_actions: usize,
    pub shutdown_grace: Duration,
    pub shutdown: Arc<Notify>,
    pub events: Arc<dyn WorkerEventSink>,
    pub result_outbox: Arc<dyn ResultOutbox>,
    pub directive_handler: Arc<dyn DirectiveHandler>,
}

/// Load plugin libraries from the supplied search paths, skipping paths which do not exist.
pub fn load_libraries(paths: &[String]) -> Result<HashMap<String, Plugin>, SendableError> {
    let mut libraries = HashMap::new();
    for path in paths {
        if !std::path::Path::new(path).exists() {
            info!(path = %path, "skipping missing plugin path");
            continue;
        }
        info!(path = %path, "loading plugins");
        libraries.extend(load_libraries_from_path(path)?);
    }
    print_libs(&libraries);
    Ok(libraries)
}

/// Run VM provider effects and supporting control/directive loops until shutdown.
pub async fn start_worker_loop(runtime: WorkerRuntime) -> Result<(), SendableError> {
    let WorkerRuntime {
        broker,
        profile,
        libraries,
        api_client,
        providers,
        max_concurrent_actions,
        shutdown_grace,
        shutdown,
        events,
        result_outbox,
        directive_handler,
    } = runtime;

    if !broker.supports_workflow_effect_channels() {
        return Err(Box::new(std::io::Error::other(
            "broker backend does not support the required VM effect channels",
        )));
    }
    if !drain_before_work(result_outbox.as_ref(), broker.as_ref(), shutdown.as_ref()).await? {
        return Ok(());
    }

    let outbox_task = {
        let broker = broker.clone();
        let result_outbox = result_outbox.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            drain_forever(result_outbox.as_ref(), broker.as_ref(), shutdown.as_ref()).await
        })
    };
    let in_flight = Arc::new(Mutex::new(HashMap::<Uuid, InFlightAction>::new()));
    let control_profile = ConsumerProfile {
        exclusive: false,
        ..profile.clone()
    };
    let control_task = tokio::spawn(run_control_loop(
        broker.clone(),
        control_profile,
        in_flight.clone(),
        shutdown.clone(),
        events.clone(),
    ));
    let restart_requested = Arc::new(AtomicBool::new(false));
    let drained = Arc::new(AtomicBool::new(false));
    let directive_task = tokio::spawn(run_directive_loop(
        broker.clone(),
        profile.clone(),
        directive_handler,
        DirectiveLoopState {
            drained: drained.clone(),
            restart_requested: restart_requested.clone(),
            state_changed: Arc::new(Notify::new()),
        },
        shutdown.clone(),
    ));

    metrics::capacity(max_concurrent_actions.max(1));
    let effect_result = crate::effect_worker::run_provider_effect_loop(
        broker,
        profile,
        libraries,
        api_client,
        providers,
        max_concurrent_actions,
        shutdown_grace,
        in_flight.clone(),
        result_outbox,
        shutdown,
        events,
        drained,
    )
    .await;

    cancel_in_flight(&in_flight).await;
    control_task.abort();
    directive_task.abort();
    outbox_task.abort();
    for (name, task) in [
        ("control", control_task),
        ("directive", directive_task),
        ("result outbox", outbox_task),
    ] {
        if let Err(error) = task.await
            && !error.is_cancelled()
        {
            error!(%error, "worker {name} task join failed");
        }
    }
    effect_result?;
    if restart_requested.load(Ordering::SeqCst) {
        return Err(Box::new(std::io::Error::other(
            "agent restart directive requested a reconnect",
        )));
    }
    Ok(())
}

async fn cancel_in_flight(in_flight: &Arc<Mutex<HashMap<Uuid, InFlightAction>>>) {
    let effects = {
        let guard = in_flight.lock().await;
        guard.values().cloned().collect::<Vec<_>>()
    };
    if !effects.is_empty() {
        warn!(
            count = effects.len(),
            "canceling in-flight provider effect(s) during shutdown"
        );
        for effect in effects {
            effect.token.cancel();
        }
    }
}

async fn run_control_loop(
    broker: Arc<dyn Broker>,
    profile: ConsumerProfile,
    in_flight: Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    shutdown: Arc<Notify>,
    events: Arc<dyn WorkerEventSink>,
) -> Result<(), SendableError> {
    let consumer_id = profile.id.clone();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => return Ok(()),
            result = broker.receive_control_for(&profile) => match result {
                Ok(delivery) => delivery,
                Err(error @ BrokerError::Unauthorized(_)) => return Err(broker_error("receive_control", error)),
                Err(error) => {
                    error!(error_code = error_code_or_unknown(&error), %error, "failed to receive control command");
                    tokio::select! {
                        _ = shutdown.notified() => return Ok(()),
                        _ = tokio::time::sleep(RECEIVE_RETRY_BACKOFF) => continue,
                    }
                }
            }
        };
        if let Err(error) =
            handle_control_delivery(&broker, &consumer_id, &in_flight, &events, delivery).await
        {
            error!(error_code = error_code_or_unknown(error.as_ref()), %error, "failed to handle control delivery");
        }
    }
}

async fn handle_control_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    in_flight: &Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    events: &Arc<dyn WorkerEventSink>,
    delivery: ControlDelivery,
) -> Result<(), SendableError> {
    let command = delivery.command;
    let control_kind = command.kind;
    metrics::control_command(match control_kind {
        ControlKind::Cancel => "cancel",
        ControlKind::Pause => "pause",
        ControlKind::Resume => "resume",
    });
    events.handle(WorkerEvent::ControlReceived {
        kind: control_kind,
        workflow_run_id: command.workflow_run_id,
    });
    if control_kind == ControlKind::Cancel {
        let effects: Vec<InFlightAction> = {
            let guard = in_flight.lock().await;
            match command.effect_id {
                Some(effect_id) => guard.get(&effect_id).cloned().into_iter().collect(),
                None => guard
                    .values()
                    .filter(|effect| effect.workflow_run_id == command.workflow_run_id)
                    .cloned()
                    .collect(),
            }
        };
        for effect in &effects {
            effect.canceled_by_control.store(true, Ordering::Release);
            effect.token.cancel();
        }
        info!(run_id = %command.workflow_run_id, effect_id = ?command.effect_id, canceled = effects.len(), "processed provider-effect cancellation");
    }
    broker
        .ack_control(consumer_id, delivery.delivery_id)
        .await
        .map_err(|error| broker_error("ack_control", error))
}
