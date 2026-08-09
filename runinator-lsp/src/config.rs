//! lsp configuration resolved from `initializationOptions` / `workspace/didChangeConfiguration`.

use serde_json::Value;

/// effective server configuration. `service_url` targets the auto-apply import; metadata
/// completion uses the process-level `RUNINATOR_API_BASE_URL` instead.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub auto_apply: bool,
    pub service_url: Option<String>,
}

impl Config {
    /// parse a settings object. accepts both a nested `{ "runinator": { ... } }` envelope and a
    /// flat object so it works with either an `initializationOptions` blob or a scoped
    /// `didChangeConfiguration` payload.
    pub fn from_value(value: Option<&Value>) -> Self {
        let mut config = Config::default();
        let Some(root) = value else {
            return config;
        };
        let scope = root.get("runinator").unwrap_or(root);
        if let Some(auto_apply) = scope.get("autoApply").and_then(Value::as_bool) {
            config.auto_apply = auto_apply;
        }
        if let Some(url) = scope.get("serviceUrl").and_then(Value::as_str)
            && !url.trim().is_empty()
        {
            config.service_url = Some(url.to_string());
        }
        config
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
