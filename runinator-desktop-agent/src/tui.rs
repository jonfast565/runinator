//! Terminal host for the desktop agent.
//!
//! This is deliberately a third host shape rather than a variation of the tray app: it owns an
//! interactive terminal, uses the same shared runtime dashboard as the service binaries, and keeps
//! the desktop-only providers and exclusive `pool=desktop` registration from the GUI/headless
//! hosts. The dashboard's normal quit keys request the same graceful `AgentHandle::stop` path as a
//! Ctrl-C in headless mode.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use runinator_models::errors::SendableError;
use runinator_observability::tui;
use runinator_worker::agent::{AgentConnection, AgentObserver, AgentRuntime, AgentStatus};
use tokio::sync::watch;
use tracing::{error, info};

use crate::cli::CliArgs;
use crate::config::AgentConfig;

/// Run the terminal dashboard host. `main` has already called [`tui::prepare`] and verified that
/// stdout is interactive before reaching here.
pub fn run(args: &CliArgs, config: AgentConfig) -> Result<(), SendableError> {
    crate::logging::init_tui(config.log_level);
    info!(
        service_url = %config.service_url,
        sandbox_root = %config.sandbox_root,
        labels = ?config.extra_labels,
        "desktop agent starting with terminal dashboard"
    );
    let _ = args;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| crate::errors::RUNTIME_BUILD.error(err))?;
    runtime.block_on(serve(config))
}

async fn serve(config: AgentConfig) -> Result<(), SendableError> {
    // The desktop providers read these at execution time; set them before the lifecycle starts.
    crate::agent::configure_provider_environment(&config);

    let grace = Duration::from_secs(config.shutdown_grace_seconds.max(1) + 5);
    let mut runtime_config = crate::agent::runtime_config(&config)?;
    let broker_description = runtime_config.broker_description.clone();

    let dashboard = tui::install();
    tui::register(
        "desktop agent",
        [
            format!("service {}", config.service_url),
            format!("broker {broker_description}"),
            format!("sandbox {}", display_sandbox(&config.sandbox_root)),
        ],
    );
    tui::activity("desktop agent", "preparing credentials", None);
    tui::register(
        "worker",
        [
            "exclusive pool=desktop".to_string(),
            format!(
                "up to {} concurrent actions",
                config.max_concurrent_actions.max(1)
            ),
        ],
    );
    tui::activity("worker", "waiting for desktop work", None);
    tui::gauge(
        "worker",
        "action capacity",
        config.max_concurrent_actions.max(1) as i64,
    );

    let stopping = Arc::new(AtomicBool::new(false));
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let dashboard_stopping = stopping.clone();
    let dashboard_request_stopping = stopping.clone();
    let dashboard_request_sender = shutdown_sender.clone();
    let dashboard_thread = tui::spawn(
        dashboard,
        move || dashboard_stopping.load(Ordering::Acquire),
        move || {
            dashboard_request_stopping.store(true, Ordering::Release);
            let _ = dashboard_request_sender.send(true);
        },
    );

    if let Err(err) = runinator_worker::prepare_agent_credentials(&mut runtime_config).await {
        finish_dashboard(&stopping, &shutdown_sender, dashboard_thread);
        return Err(err);
    }
    let mut agent = match AgentRuntime::start(runtime_config, Arc::new(DashboardObserver)) {
        Ok(agent) => agent,
        Err(err) => {
            finish_dashboard(&stopping, &shutdown_sender, dashboard_thread);
            return Err(err);
        }
    };

    let result = tokio::select! {
        _ = wait_for_dashboard_shutdown(shutdown_receiver) => {
            info!("terminal dashboard requested shutdown, stopping desktop agent");
            agent.stop(grace).await
        }
        signal = tokio::signal::ctrl_c() => {
            match signal {
                Ok(()) => {
                    info!("shutdown signal received, stopping desktop agent");
                    agent.stop(grace).await
                }
                Err(err) => Err(crate::errors::SIGNAL_CTRL_C.error(err)),
            }
        }
        result = agent.wait() => result,
    };
    finish_dashboard(&stopping, &shutdown_sender, dashboard_thread);
    report_exit(result)
}

/// Host observer that supplements the worker-loop metrics with the lifecycle phase. Lifecycle log
/// messages themselves already travel through tracing into the dashboard's rolling log pane, so
/// this intentionally does not forward `on_log` and duplicate every line.
struct DashboardObserver;

impl AgentObserver for DashboardObserver {
    fn on_status(&self, status: &AgentStatus) {
        tui::activity("desktop agent", status_summary(status), None);
        tui::gauge(
            "desktop agent",
            "actions in flight",
            status.metrics.in_flight as i64,
        );
        tui::gauge(
            "desktop agent",
            "actions succeeded",
            status.metrics.succeeded.min(i64::MAX as u64) as i64,
        );
        tui::gauge(
            "desktop agent",
            "actions failed",
            status.metrics.failed.min(i64::MAX as u64) as i64,
        );
    }
}

fn status_summary(status: &AgentStatus) -> String {
    match &status.connection {
        AgentConnection::Reconnecting {
            retry_secs,
            attempt,
            max_attempts,
        } => match max_attempts {
            Some(max) => format!("reconnecting ({attempt}/{max}; retry in {retry_secs}s)"),
            None => format!("reconnecting (retry in {retry_secs}s)"),
        },
        AgentConnection::Disconnected { attempts, reason } => {
            format!("disconnected after {attempts} attempts: {reason}")
        }
        AgentConnection::ReenrollmentRequired { reason } => {
            format!("re-enrollment required: {reason}")
        }
        connection => connection.as_str().to_string(),
    }
}

fn display_sandbox(path: &str) -> &str {
    if path.trim().is_empty() {
        "not configured"
    } else {
        path
    }
}

async fn wait_for_dashboard_shutdown(mut receiver: watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

fn finish_dashboard(
    stopping: &AtomicBool,
    shutdown_sender: &watch::Sender<bool>,
    dashboard_thread: std::thread::JoinHandle<()>,
) {
    stopping.store(true, Ordering::Release);
    let _ = shutdown_sender.send(true);
    let _ = dashboard_thread.join();
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
