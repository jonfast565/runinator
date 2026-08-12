//! command-line and environment overrides for the persisted agent config.
//!
//! precedence is **cli > env > json file > defaults**. clap gives the first two (every override is
//! an `Option` with an `env` fallback); [`CliArgs::apply`] gives the third by leaving a `None`
//! override alone, so an operator can pin one setting on the command line without having to restate
//! the rest of a config someone already filled in through the gui.

use clap::Parser;

use crate::config::{AgentConfig, BrokerMode, LogLevel};

#[derive(Parser, Debug, Default)]
#[command(author, version, about = "Runinator Desktop Agent", long_about = None)]
pub struct CliArgs {
    /// run without the tray/window ui. for a machine that should join the cluster on boot and be
    /// managed remotely — the same lifecycle the gui drives, with no desktop session required.
    #[arg(long, env = "RUNINATOR_AGENT_HEADLESS")]
    pub headless: bool,

    /// web service url this agent registers with, and (in relay mode) tunnels the broker through.
    #[arg(long, env = "RUNINATOR_SERVICE_URL")]
    pub service_url: Option<String>,

    /// service api key presented to the web service.
    #[arg(long, env = "RUNINATOR_API_KEY")]
    pub api_key: Option<String>,

    /// comma-separated extra routing labels, e.g. `runner=creds-sync,zone=onprem`. `pool=desktop`
    /// is always advertised in addition to these.
    #[arg(long, env = "RUNINATOR_WORKER_LABELS")]
    pub labels: Option<String>,

    /// folder the local-files provider is confined to.
    #[arg(long, env = "RUNINATOR_LOCAL_FILES_ROOT")]
    pub sandbox_root: Option<String>,

    /// allow the local-files provider to write inside the sandbox.
    #[arg(long, env = "RUNINATOR_LOCAL_FILES_ALLOW_WRITE")]
    pub allow_write: bool,

    /// base directory `console.run` commands execute from.
    #[arg(long, env = "RUNINATOR_CONSOLE_WORKING_DIR")]
    pub console_working_dir: Option<String>,

    /// how to reach the broker: `relay` (through the web service) or `direct`.
    #[arg(long, env = "RUNINATOR_BROKER_MODE")]
    pub broker_mode: Option<String>,

    /// broker backend name, only used with `--broker-mode direct`.
    #[arg(long, env = "RUNINATOR_BROKER_BACKEND")]
    pub direct_broker_backend: Option<String>,

    /// broker endpoint, only used with `--broker-mode direct`.
    #[arg(long, env = "RUNINATOR_BROKER_ENDPOINT")]
    pub direct_broker_endpoint: Option<String>,

    /// how many actions this agent runs at once.
    #[arg(long, env = "RUNINATOR_MAX_CONCURRENT_ACTIONS")]
    pub max_concurrent_actions: Option<usize>,

    /// seconds to wait for in-flight actions on shutdown.
    #[arg(long, env = "RUNINATOR_SHUTDOWN_GRACE_SECONDS")]
    pub shutdown_grace_seconds: Option<u64>,

    /// path to a file touched periodically while the agent is alive; empty disables it.
    #[arg(long, env = "RUNINATOR_LIVENESS_FILE")]
    pub liveness_file: Option<String>,

    /// tracing verbosity: error, warn, info, debug, or trace. `RUST_LOG` still wins.
    #[arg(long, env = "RUNINATOR_AGENT_LOG_LEVEL")]
    pub log_level: Option<String>,
}

impl CliArgs {
    /// overlay these overrides onto a config loaded from disk. unrecognized enum spellings are
    /// ignored rather than fatal: a typo in an env var should not stop an unattended machine from
    /// coming up with its last known-good settings.
    pub fn apply(&self, mut config: AgentConfig) -> AgentConfig {
        overlay(&mut config.service_url, self.service_url.as_deref());
        overlay(&mut config.sandbox_root, self.sandbox_root.as_deref());
        overlay(
            &mut config.console_working_dir,
            self.console_working_dir.as_deref(),
        );
        overlay(
            &mut config.direct_broker_backend,
            self.direct_broker_backend.as_deref(),
        );
        overlay(
            &mut config.direct_broker_endpoint,
            self.direct_broker_endpoint.as_deref(),
        );
        overlay(&mut config.liveness_file, self.liveness_file.as_deref());

        if let Some(api_key) = non_blank(self.api_key.as_deref()) {
            config.api_key = Some(api_key.to_string());
        }
        if let Some(labels) = self.labels.as_deref() {
            config.extra_labels = labels
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(str::to_string)
                .collect();
        }
        // a bare `--allow-write` can only turn writing on; the persisted value stands otherwise, so
        // the flag never silently revokes what the operator granted in the gui.
        if self.allow_write {
            config.allow_write = true;
        }
        if let Some(mode) = self.broker_mode.as_deref().and_then(BrokerMode::parse) {
            config.broker_mode = mode;
        }
        if let Some(value) = self.max_concurrent_actions {
            config.max_concurrent_actions = value.max(1);
        }
        if let Some(value) = self.shutdown_grace_seconds {
            config.shutdown_grace_seconds = value.max(1);
        }
        if let Some(level) = self.log_level.as_deref().and_then(LogLevel::parse) {
            config.log_level = level;
        }
        config
    }
}

fn overlay(target: &mut String, value: Option<&str>) {
    if let Some(value) = non_blank(value) {
        *target = value.to_string();
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
