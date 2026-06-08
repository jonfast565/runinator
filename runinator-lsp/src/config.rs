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
        if let Some(url) = scope.get("serviceUrl").and_then(Value::as_str) {
            if !url.trim().is_empty() {
                config.service_url = Some(url.to_string());
            }
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_nested_envelope() {
        let value = json!({ "runinator": { "autoApply": true, "serviceUrl": "http://x/" } });
        let config = Config::from_value(Some(&value));
        assert!(config.auto_apply);
        assert_eq!(config.service_url.as_deref(), Some("http://x/"));
    }

    #[test]
    fn parses_flat_object() {
        let value = json!({ "autoApply": true });
        let config = Config::from_value(Some(&value));
        assert!(config.auto_apply);
        assert!(config.service_url.is_none());
    }

    #[test]
    fn empty_service_url_is_none() {
        let value = json!({ "serviceUrl": "   " });
        assert!(Config::from_value(Some(&value)).service_url.is_none());
    }

    #[test]
    fn missing_options_default_off() {
        let config = Config::from_value(None);
        assert!(!config.auto_apply);
        assert!(config.service_url.is_none());
    }
}
