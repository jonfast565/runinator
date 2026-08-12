use std::{env, ffi::OsString, sync::Arc, time::Duration};

use runinator_models::errors::SendableError;
use runinator_utilities::startup;
use tracing::{error, info};

use runinator_worker::{AgentRuntime, Config, NoopObserver, errors, parse_config};

mod service;
use service::WorkerService;

#[cfg(test)]
mod main_tests;

fn main() -> Result<(), SendableError> {
    WorkerService::new().run()
}

fn run_process() -> Result<(), SendableError> {
    // held for the process lifetime so otel signals flush on shutdown.
    let _telemetry = startup::startup("Runinator Worker")?;

    let config = parse_config()?;
    configure_provider_service_url(&config);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| errors::RUNTIME_BUILD.error(err))?;
    runtime.block_on(run(config))
}

async fn run(config: Config) -> Result<(), SendableError> {
    // log the advertised routing labels: a label-targeted action (e.g. a `.runner("creds-sync")`
    // node) only lands here when these satisfy its selector, so surfacing them makes "which worker
    // did this go to" answerable from the worker's own log.
    info!(
        worker_id = %config.worker_id,
        labels = ?config.labels,
        "worker starting"
    );

    // the shared agent lifecycle owns registration retry, heartbeat, and restarting the action loop
    // after a failure; tracing already reports loop activity here, so no observer is needed.
    let shutdown_grace = Duration::from_secs(config.shutdown_grace_seconds + 5);
    let mut runtime_config = config.agent_runtime_config()?;
    runinator_worker::prepare_agent_credentials(&mut runtime_config).await?;
    let mut agent = AgentRuntime::start(runtime_config, Arc::new(NoopObserver))?;

    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|err| errors::SIGNAL_CTRL_C.error(err))?;
        }
        // the lifecycle only returns on its own when it could not be brought up at all; propagate
        // that so the process exits non-zero and its orchestrator restarts it.
        result = agent.wait() => {
            return report_exit(result);
        }
    }

    info!("shutdown signal received, stopping worker");
    report_exit(agent.stop(shutdown_grace).await)
}

fn report_exit(result: Result<(), SendableError>) -> Result<(), SendableError> {
    let Err(err) = result else {
        return Ok(());
    };
    error!(
        error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
        "worker stopped with error: {}", err
    );
    Err(err)
}

fn configure_provider_service_url(config: &Config) {
    let Some(value) =
        provider_service_url_fallback(env::var_os("RUNINATOR_SERVICE_URL"), &config.api_base_url)
    else {
        return;
    };

    // safety: this runs before the worker starts provider execution or spawns runtime work.
    unsafe {
        env::set_var("RUNINATOR_SERVICE_URL", value);
    }
}

fn provider_service_url_fallback(
    existing: Option<OsString>,
    api_base_url: &str,
) -> Option<OsString> {
    if existing
        .as_ref()
        .is_some_and(|value| !value.to_string_lossy().trim().is_empty())
    {
        return None;
    }
    Some(OsString::from(api_base_url))
}
