//! persisted agent settings: the last-used service url and sandbox folder, so the GUI form does not
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
    /// relay through `runinator-ws`'s `/ws/desktop-worker` endpoint (derived from `service_url`).
    /// the safe default for a machine that shouldn't (or can't) reach the broker directly.
    #[default]
    Relay,
    /// connect straight to a broker backend (`direct_broker_backend`/`direct_broker_endpoint`) —
    /// for a machine that's actually on the trusted network and wants to skip the relay hop.
    Direct,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// also used to derive the ws broker relay URL in `BrokerMode::Relay` (scheme swapped,
    /// `/ws/desktop-worker` appended) — see `agent::derive_relay_url`.
    pub service_url: String,
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
    /// extra routing labels this replica advertises, beyond the always-on `pool=desktop` — each
    /// entry a `key=value` tag (same pairs `RUNINATOR_WORKER_LABELS`/`runinator_worker::parse_labels`
    /// accept, joined with commas before parsing), so any future workflow that needs to pin work to a
    /// desktop instance just needs a matching `.runner("...")`/label requirement, with no new agent
    /// code or GUI control required.
    #[serde(default)]
    pub extra_labels: Vec<String>,
    #[serde(default)]
    pub broker_mode: BrokerMode,
    /// broker backend name (`tcp`/`rabbitmq`/`kafka`/`http`), only used in `BrokerMode::Direct`.
    #[serde(default = "default_direct_broker_backend")]
    pub direct_broker_backend: String,
    /// broker endpoint, only used in `BrokerMode::Direct` (e.g. `host:port` for tcp,
    /// `amqp://user:pass@host:port/%2f` for rabbitmq).
    #[serde(default)]
    pub direct_broker_endpoint: String,
    /// the command-center UI's URL, opened in the system's default browser by the "Open UI" button
    /// (and tray menu item) when `command_center_app_path` is empty. a separate field from
    /// `service_url`: the UI is typically its own deployment/ingress, not reachable by swapping a
    /// path on the ws API's URL.
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
    /// verbosity of the tracing output shown in the in-app log console; the GUI dropdown changes it
    /// live (`RUST_LOG`, if set, still wins at process startup).
    #[serde(default)]
    pub log_level: LogLevel,
}

fn default_direct_broker_backend() -> String {
    "tcp".to_string()
}

fn default_max_concurrent_actions() -> usize {
    2
}

fn default_shutdown_grace_seconds() -> u64 {
    10
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            service_url: "http://127.0.0.1:8080/".to_string(),
            sandbox_root: String::new(),
            console_working_dir: String::new(),
            allow_write: false,
            api_key: None,
            extra_labels: Vec::new(),
            broker_mode: BrokerMode::default(),
            direct_broker_backend: default_direct_broker_backend(),
            direct_broker_endpoint: String::new(),
            command_center_url: String::new(),
            command_center_app_path: String::new(),
            auto_start: false,
            max_concurrent_actions: default_max_concurrent_actions(),
            shutdown_grace_seconds: default_shutdown_grace_seconds(),
            log_level: LogLevel::default(),
        }
    }
}

/// load the last-saved config, falling back to defaults on any error (no file yet, bad json, ...).
pub fn load() -> AgentConfig {
    runinator_utilities::app_data::app_data_path(CONFIG_FILE_NAME)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// best-effort save; a failure here should never block the caller (e.g. starting the agent).
pub fn save(config: &AgentConfig) {
    let Ok(path) = runinator_utilities::app_data::app_data_path(CONFIG_FILE_NAME) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, raw);
    }
}
