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

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::errors::SendableError;
use runinator_observability::tui;
use runinator_worker::agent::{AgentConnection, AgentObserver, AgentRuntime, AgentStatus};
use tokio::sync::{mpsc, watch};
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
    tui::register("enrollment", initial_enrollment_details(&runtime_config));
    tui::register("execution profiles", execution_profile_details(&[], 0));
    tui::activity("execution profiles", "waiting for agent credentials", None);
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
    let (profile_command_sender, profile_command_receiver) = mpsc::unbounded_channel();
    let dashboard_stopping = stopping.clone();
    let dashboard_request_stopping = stopping.clone();
    let dashboard_request_sender = shutdown_sender.clone();
    let dashboard_profile_commands = profile_command_sender.clone();
    let dashboard_thread = tui::spawn_with_key_handler(
        dashboard,
        move || dashboard_stopping.load(Ordering::Acquire),
        move || {
            dashboard_request_stopping.store(true, Ordering::Release);
            let _ = dashboard_request_sender.send(true);
        },
        move |key| {
            let command = match key {
                '[' => Some(ProfileCommand::SelectPrevious),
                ']' => Some(ProfileCommand::SelectNext),
                'a' => Some(ProfileCommand::Approve),
                'r' => Some(ProfileCommand::Revoke),
                _ => None,
            };
            command.is_some_and(|command| dashboard_profile_commands.send(command).is_ok())
        },
    );

    if let Err(err) = runinator_worker::prepare_agent_credentials(&mut runtime_config).await {
        finish_dashboard(&stopping, &shutdown_sender, dashboard_thread);
        return Err(err);
    }
    let profile_client = match AsyncApiClient::with_credentials(
        StaticLocator::new(runtime_config.service_url.clone()),
        runtime_config.api_key.clone(),
    ) {
        Ok(client) => Some(client),
        Err(error) => {
            tui::register(
                "execution profiles",
                [format!("Unable to load execution profiles: {error}")],
            );
            tui::activity("execution profiles", "profile controls unavailable", None);
            None
        }
    };
    let mut agent = match AgentRuntime::start(runtime_config, Arc::new(DashboardObserver)) {
        Ok(agent) => agent,
        Err(err) => {
            finish_dashboard(&stopping, &shutdown_sender, dashboard_thread);
            return Err(err);
        }
    };
    let _profile_task = profile_client.map(|client| {
        spawn_execution_profile_sync(client, agent.watch(), profile_command_receiver)
    });

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

#[derive(Clone, Copy)]
enum ProfileCommand {
    SelectPrevious,
    SelectNext,
    Approve,
    Revoke,
}

fn spawn_execution_profile_sync(
    client: AsyncApiClient<StaticLocator>,
    mut agent: watch::Receiver<AgentStatus>,
    mut commands: mpsc::UnboundedReceiver<ProfileCommand>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut profiles = Vec::new();
        let mut selected = 0;
        let mut refresh = true;

        loop {
            if agent.borrow().connection == AgentConnection::Stopped {
                return;
            }
            if refresh {
                tui::activity("execution profiles", "checking local approvals", None);
                match crate::execution_profiles::synchronize(&client, tui::log_line).await {
                    Ok(next) => {
                        profiles = next;
                        selected = selected.min(profiles.len().saturating_sub(1));
                        register_execution_profiles(&profiles, selected);
                    }
                    Err(error) => {
                        tui::register(
                            "execution profiles",
                            [format!("Synchronization failed: {error}")],
                        );
                        tui::activity("execution profiles", "synchronization failed", None);
                        tui::log_line(format!("Execution profile synchronization failed: {error}"));
                    }
                }
            }

            refresh = tokio::select! {
                Some(command) = commands.recv() => {
                    let refresh = handle_profile_command(&mut profiles, &mut selected, command);
                    if !refresh {
                        register_execution_profiles(&profiles, selected);
                    }
                    refresh
                }
                changed = agent.changed() => {
                    if changed.is_err() || agent.borrow().connection == AgentConnection::Stopped {
                        return;
                    }
                    true
                }
                _ = tokio::time::sleep(crate::execution_profiles::PROFILE_SYNC_INTERVAL) => {
                    true
                }
            };
        }
    })
}

fn handle_profile_command(
    profiles: &mut [crate::execution_profiles::LocalProfileStatus],
    selected: &mut usize,
    command: ProfileCommand,
) -> bool {
    if profiles.is_empty() {
        tui::log_line("No execution profiles are available yet.");
        return false;
    }

    match command {
        ProfileCommand::SelectPrevious => {
            *selected = selected.checked_sub(1).unwrap_or(profiles.len() - 1);
            false
        }
        ProfileCommand::SelectNext => {
            *selected = (*selected + 1) % profiles.len();
            false
        }
        ProfileCommand::Approve => {
            let profile = &profiles[*selected];
            if !profile.enabled {
                tui::log_line(format!(
                    "Execution profile '{}' is disabled centrally and cannot be approved.",
                    profile.name
                ));
                return false;
            }
            let mut config = crate::config::load();
            if config.approved_execution_profiles.get(&profile.id) == Some(&profile.config_digest) {
                tui::log_line(format!(
                    "Execution profile '{}' is already approved for this configuration.",
                    profile.name
                ));
                return false;
            }
            config
                .approved_execution_profiles
                .insert(profile.id, profile.config_digest.clone());
            if crate::config::save(&config) {
                tui::log_line(format!(
                    "Approved execution profile '{}' locally; synchronizing collection.",
                    profile.name
                ));
                true
            } else {
                tui::log_line(format!(
                    "Could not save local approval for execution profile '{}'.",
                    profile.name
                ));
                false
            }
        }
        ProfileCommand::Revoke => {
            let profile = &profiles[*selected];
            let mut config = crate::config::load();
            if config
                .approved_execution_profiles
                .remove(&profile.id)
                .is_none()
            {
                tui::log_line(format!(
                    "Execution profile '{}' has no saved local approval.",
                    profile.name
                ));
                return false;
            }
            if crate::config::save(&config) {
                tui::log_line(format!(
                    "Revoked local approval for execution profile '{}'.",
                    profile.name
                ));
                true
            } else {
                tui::log_line(format!(
                    "Could not save local revocation for execution profile '{}'.",
                    profile.name
                ));
                false
            }
        }
    }
}

fn register_execution_profiles(
    profiles: &[crate::execution_profiles::LocalProfileStatus],
    selected: usize,
) {
    tui::register(
        "execution profiles",
        execution_profile_details(profiles, selected),
    );
    let activity = profiles
        .get(selected)
        .map(|profile| profile.message.clone())
        .unwrap_or_else(|| "waiting for centrally configured profiles".to_string());
    tui::activity("execution profiles", activity, None);
    tui::gauge(
        "execution profiles",
        "configured profiles",
        profiles.len() as i64,
    );
}

fn execution_profile_details(
    profiles: &[crate::execution_profiles::LocalProfileStatus],
    selected: usize,
) -> Vec<String> {
    let Some(profile) = profiles.get(selected) else {
        return vec![
            "No centrally configured execution profiles are available yet.".to_string(),
            "[/] select · a approve selected profile · r revoke selected profile".to_string(),
        ];
    };
    let approval = if !profile.enabled {
        "disabled centrally"
    } else if profile.approved {
        "approved on this computer"
    } else {
        "not approved on this computer"
    };
    vec![
        format!("{}/{}: {}", selected + 1, profiles.len(), profile.name),
        approval.to_string(),
        format!(
            "config {} · collection {}",
            &profile.config_digest[..profile.config_digest.len().min(12)],
            profile.message
        ),
        "[/] select · a approve selected profile · r revoke selected profile".to_string(),
    ]
}

/// Host observer that supplements the worker-loop metrics with the lifecycle phase. Lifecycle log
/// messages themselves already travel through tracing into the dashboard's rolling log pane, so
/// this intentionally does not forward `on_log` and duplicate every line.
struct DashboardObserver;

impl AgentObserver for DashboardObserver {
    fn on_status(&self, status: &AgentStatus) {
        tui::activity("desktop agent", status_summary(status), None);
        tui::register("enrollment", enrollment_details(&status.connection));
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
            format!("re-enrollment required: {reason}; restart with --enroll <token>")
        }
        connection => connection.as_str().to_string(),
    }
}

/// The TUI cannot safely collect a secret in raw-mode/alternate-screen input, so it provides the
/// equivalent recovery process directly in its enrollment card. `--enroll` works on a first start
/// and intentionally supersedes the cached credential after the broker rejects it.
fn initial_enrollment_details(
    runtime_config: &runinator_worker::agent::AgentRuntimeConfig,
) -> Vec<String> {
    if runtime_config.enrollment_token.is_some() {
        vec![
            "Redeeming the supplied one-time enrollment token".to_string(),
            "The token is not saved after enrollment".to_string(),
        ]
    } else if runtime_config.api_key.is_some() {
        vec!["Using the configured API key".to_string()]
    } else if runtime_config.credential_file.is_file() {
        vec!["Using the saved desktop-agent credential".to_string()]
    } else {
        vec![
            "First-time enrollment required".to_string(),
            "Create a token in Command Center: Replicas → Enroll a machine".to_string(),
            "Restart with: runinator-desktop-agent --tui --enroll <token>".to_string(),
        ]
    }
}

fn enrollment_details(connection: &AgentConnection) -> Vec<String> {
    match connection {
        AgentConnection::ReenrollmentRequired { reason } => vec![
            "Saved broker credential was rejected".to_string(),
            format!("Reason: {reason}"),
            "Create a token in Command Center: Replicas → Enroll a machine".to_string(),
            "Quit, then restart: runinator-desktop-agent --tui --enroll <token>".to_string(),
        ],
        AgentConnection::Connected => {
            vec!["Credential accepted; broker access is active".to_string()]
        }
        AgentConnection::Registering => vec!["Preparing the agent credential".to_string()],
        AgentConnection::Connecting | AgentConnection::Reconnecting { .. } => {
            vec!["Credential prepared; connecting to the broker".to_string()]
        }
        AgentConnection::Disconnected { .. } => {
            vec!["Credential retained; broker connection is stopped".to_string()]
        }
        AgentConnection::Stopped => vec!["Agent is stopped".to_string()],
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

#[cfg(test)]
#[path = "tui_tests.rs"]
mod tests;
