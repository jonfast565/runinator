//! persisted agent settings: the last-used service URL and sandbox folder, so the GUI form does not
//! need to be re-filled on every launch. best-effort only; a missing or corrupt file falls back to
//! defaults rather than blocking startup.

use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "desktop-agent.json";

/// which broker transport this agent uses — orthogonal to it being a "desktop" worker: a cloud
/// worker can just as well relay through `runinator-ws` (e.g. no direct network path to the
/// broker), and a desktop machine on the trusted network can just as well connect directly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrokerMode {
    /// relay through `runinator-ws`'s `/ws/broker` endpoint (derived from `service_url`).
    /// the safe default for a machine that shouldn't (or can't) reach the broker directly.
    #[default]
    Relay,
    /// connect straight to a broker backend (`direct_broker_backend`/`direct_broker_endpoint`) —
    /// for a machine that's actually on the trusted network and wants to skip the relay hop.
    Direct,
}

// the persisted form is this crate's, because the shared runtime carries no serde; the runtime form
// is the shared one, so there is exactly one place that decides what "relay" means.
impl From<BrokerMode> for runinator_worker::BrokerMode {
    fn from(mode: BrokerMode) -> Self {
        match mode {
            BrokerMode::Relay => runinator_worker::BrokerMode::Relay,
            BrokerMode::Direct => runinator_worker::BrokerMode::Direct,
        }
    }
}

impl BrokerMode {
    /// parse a CLI/env spelling; `None` when unrecognized so a caller can keep the persisted value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "relay" => Some(BrokerMode::Relay),
            "direct" => Some(BrokerMode::Direct),
            _ => None,
        }
    }
}

/// verbosity for the agent's tracing output, surfaced live in the in-app log console. maps to a
/// tracing `EnvFilter` base level (see `crate::logging`); the GUI dropdown drives it at runtime.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// levels in increasing-verbosity order, for the GUI dropdown.
    pub const ALL: [LogLevel; 5] = [
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];

    /// parse a CLI/env spelling; `None` when unrecognized so a caller can keep the persisted value.
    pub fn parse(raw: &str) -> Option<Self> {
        LogLevel::ALL
            .into_iter()
            .find(|level| level.as_str() == raw.trim().to_ascii_lowercase())
    }

    /// the lowercase name, both the serde form and the tracing filter directive.
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

/// The action to take when the operator closes the main window after opting out of the exit
/// confirmation. `None` means continue asking.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseAction {
    HideToTray,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// also used to derive the WS broker relay URL in `BrokerMode::Relay` (scheme swapped,
    /// `/ws/broker` appended) — see `agent::derive_relay_url`.
    pub service_url: String,
    #[serde(default)]
    pub discover: bool,
    #[serde(default = "default_gossip_bind")]
    pub gossip_bind: String,
    #[serde(default = "default_gossip_port")]
    pub gossip_port: u16,
    pub sandbox_root: String,
    /// base directory `console.run` commands execute from on this machine (the child process's
    /// `current_dir`), exported to the console provider as `RUNINATOR_CONSOLE_WORKING_DIR`. lets a
    /// workflow reference files by a relative path (e.g. `bash scripts/sync-secrets.sh` from a repo
    /// checkout) instead of an absolute one baked in at import. empty inherits the agent's own cwd.
    #[serde(default)]
    pub console_working_dir: String,
    #[serde(default)]
    pub allow_write: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    /// ephemeral first-start input; explicitly excluded from persisted JSON.
    #[serde(skip)]
    pub enrollment_token: Option<String>,
    /// extra routing labels this replica advertises, beyond the always-on `pool=desktop` — each
    /// entry a `key=value` tag (same pairs `RUNINATOR_WORKER_LABELS`/`runinator_worker::parse_labels`
    /// accept, joined with commas before parsing), so any future workflow that needs to pin work to a
    /// desktop instance just needs a matching `.runner("...")`/label requirement, with no new agent
    /// code or GUI control required.
    #[serde(default = "default_extra_labels")]
    pub extra_labels: Vec<String>,
    #[serde(default)]
    pub broker_mode: BrokerMode,
    /// Broker backend name (`tcp`/`rabbitmq`/`kafka`/`http`), used only in `Direct`.
    #[serde(default = "default_direct_broker_backend")]
    pub direct_broker_backend: String,
    /// Broker endpoint, used only in `BrokerMode::Direct` (for example, `host:port` for tcp,
    /// `amqp://user:pass@host:port/%2f` for rabbitmq).
    #[serde(default)]
    pub direct_broker_endpoint: String,
    /// the command-center UI's URL, opened in the system's default browser by the "Open UI" button
    /// (and tray menu item) when `command_center_app_path` is empty. a separate field from
    /// `service_url`: the UI is typically its own deployment/ingress, not reachable by swapping a
    /// path on the WS API's URL.
    #[serde(default)]
    pub command_center_url: String,
    /// path to a native command-center install (a Tauri `.app` bundle on macOS, or an executable on
    /// Windows/Linux) — "Open UI" launches this directly instead of the browser URL when set.
    #[serde(default)]
    pub command_center_app_path: String,
    /// start the agent immediately when the process launches, without waiting for a manual click on
    /// "Start agent" — for running this as a login item/background service on a machine nobody is
    /// actively watching (e.g. the box that does hourly `packs/creds-sync` runs).
    #[serde(default)]
    pub auto_start: bool,
    /// how many actions this replica runs at once; same knob as `runinator-worker`'s
    /// `--max-concurrent-actions`.
    #[serde(default = "default_max_concurrent_actions")]
    pub max_concurrent_actions: usize,
    /// seconds the worker loop waits for in-flight actions to finish on shutdown before dropping
    /// them; same knob as `runinator-worker`'s `--shutdown-grace-seconds`.
    #[serde(default = "default_shutdown_grace_seconds")]
    pub shutdown_grace_seconds: u64,
    /// how many consecutive failed reconnects to tolerate before the agent disconnects and stops
    /// itself. unlike an in-cluster worker, this machine has no orchestrator to restart it and no
    /// reason to sit heartbeating a replica that can never take work — a laptop that left the
    /// network should drop off the registry rather than spin against a service that is not there.
    /// the count resets after a connection that stayed up, so this bounds one outage rather than the
    /// agent's lifetime; `0` retries forever.
    #[serde(default = "default_reconnect_max_attempts")]
    pub reconnect_max_attempts: u32,
    /// path touched periodically while the shared runtime is alive; empty disables the probe.
    #[serde(default)]
    pub liveness_file: String,
    /// verbosity of the tracing output shown in the in-app log console; the GUI dropdown changes it
    /// live (`RUST_LOG`, if set, still wins at process startup).
    #[serde(default)]
    pub log_level: LogLevel,
    /// remembered response to the main-window close prompt. `None` leaves the prompt enabled.
    #[serde(default)]
    pub window_close_action: Option<WindowCloseAction>,
}

fn default_direct_broker_backend() -> String {
    "tcp".to_string()
}

fn default_extra_labels() -> Vec<String> {
    vec!["runner=desktop".to_string()]
}

fn default_gossip_bind() -> String {
    "0.0.0.0".to_string()
}

fn default_gossip_port() -> u16 {
    5000
}

fn default_max_concurrent_actions() -> usize {
    2
}

fn default_shutdown_grace_seconds() -> u64 {
    10
}

fn default_reconnect_max_attempts() -> u32 {
    runinator_worker::agent::DEFAULT_RECONNECT_MAX_ATTEMPTS
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            service_url: "http://127.0.0.1:8080/".to_string(),
            discover: false,
            gossip_bind: default_gossip_bind(),
            gossip_port: default_gossip_port(),
            sandbox_root: String::new(),
            console_working_dir: String::new(),
            allow_write: false,
            api_key: None,
            enrollment_token: None,
            extra_labels: default_extra_labels(),
            broker_mode: BrokerMode::default(),
            direct_broker_backend: default_direct_broker_backend(),
            direct_broker_endpoint: String::new(),
            command_center_url: String::new(),
            command_center_app_path: String::new(),
            auto_start: false,
            max_concurrent_actions: default_max_concurrent_actions(),
            shutdown_grace_seconds: default_shutdown_grace_seconds(),
            reconnect_max_attempts: default_reconnect_max_attempts(),
            liveness_file: String::new(),
            log_level: LogLevel::default(),
            window_close_action: None,
        }
    }
}

/// load the last-saved config, falling back to defaults on any error (no file yet, bad json, ...).
pub fn load() -> AgentConfig {
    runinator_platform::app_data::app_data_path(CONFIG_FILE_NAME)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// best-effort save; a failure here should never block the caller (e.g. starting the agent).
pub fn save(config: &AgentConfig) {
    let Ok(path) = runinator_platform::app_data::app_data_path(CONFIG_FILE_NAME) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, raw);
    }
}

#[cfg(test)]
mod tests {
    use super::AgentConfig;

    #[test]
    fn defaults_to_the_desktop_runner_label() {
        assert_eq!(AgentConfig::default().extra_labels, ["runner=desktop"]);
    }

    #[test]
    fn missing_labels_receive_the_desktop_runner_label() {
        let config: AgentConfig = serde_json::from_value(serde_json::json!({
            "service_url": "http://127.0.0.1:8080/",
            "sandbox_root": ""
        }))
        .unwrap();

        assert_eq!(config.extra_labels, ["runner=desktop"]);
    }
}
