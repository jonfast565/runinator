//! the `--headless` entry point: the same agent the tray hosts, with no window, no tray, and no
//! desktop session required.
//!
//! this is what makes "run an agent on that machine over there" a supported deployment rather than a
//! gui someone has to stay logged into. it still registers as the exclusive `pool=desktop` replica
//! and still carries the local-files provider, because those are properties of *this machine*, not
//! of whether a window is open.

use std::sync::Arc;
use std::time::Duration;

use runinator_models::errors::SendableError;
use runinator_worker::agent::{AgentRuntime, NoopObserver};
use tracing::{error, info};

use crate::cli::CliArgs;
use crate::config::AgentConfig;

pub fn run(args: &CliArgs, config: AgentConfig) -> Result<(), SendableError> {
    crate::logging::init_headless(config.log_level);
    info!(
        service_url = %config.service_url,
        sandbox_root = %config.sandbox_root,
        labels = ?config.extra_labels,
        "desktop agent starting headless"
    );
    let _ = args;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| crate::errors::RUNTIME_BUILD.error(err))?;
    runtime.block_on(serve(config))
}

async fn serve(config: AgentConfig) -> Result<(), SendableError> {
    // the desktop providers read these at execution time; set before anything spawns.
    crate::agent::configure_provider_environment(&config);

    let grace = Duration::from_secs(config.shutdown_grace_seconds.max(1) + 5);
    let mut agent = AgentRuntime::start(
        crate::agent::runtime_config(&config)?,
        Arc::new(NoopObserver),
    )?;

    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|err| crate::errors::SIGNAL_CTRL_C.error(err))?;
        }
        // the lifecycle only returns on its own when it could not be brought up at all; propagate
        // that so the process exits non-zero and its supervisor restarts it.
        result = agent.wait() => {
            return report_exit(result);
        }
    }

    info!("shutdown signal received, stopping desktop agent");
    report_exit(agent.stop(grace).await)
}

fn report_exit(result: Result<(), SendableError>) -> Result<(), SendableError> {
    let Err(err) = result else {
        return Ok(());
    };
    error!(
        error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
        "desktop agent stopped with error: {}", err
    );
    Err(err)
}
