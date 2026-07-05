//! persisted agent settings: the last-used service/broker urls and sandbox folder, so the GUI form
//! does not need to be re-filled on every launch. best-effort only; a missing or corrupt file falls
//! back to defaults rather than blocking startup.

use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "desktop-agent.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub service_url: String,
    pub broker_url: String,
    pub sandbox_root: String,
    #[serde(default)]
    pub allow_write: bool,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            service_url: "http://127.0.0.1:8080/".to_string(),
            broker_url: "http://127.0.0.1:8088/".to_string(),
            sandbox_root: String::new(),
            allow_write: false,
            api_key: None,
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
