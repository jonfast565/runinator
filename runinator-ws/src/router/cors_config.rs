#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub(super) allowed_origins: Vec<HeaderValue>,
}

impl CorsConfig {
    /// Parse exact origins supplied by the CLI/environment. A wildcard is deliberately rejected:
    /// when authentication is disabled, allowing every origin turns any visited website into an
    /// administrator of the operator's local Runinator service.
    pub fn new(origins: Vec<String>) -> Result<Self, String> {
        let mut allowed_origins = Vec::new();
        for raw in origins {
            let origin = raw.trim();
            if origin.is_empty() {
                continue;
            }
            if origin == "*" {
                return Err("CORS allowed origins must be explicit; '*' is not permitted".into());
            }
            let uri = origin
                .parse::<axum::http::Uri>()
                .map_err(|err| format!("invalid CORS origin '{origin}': {err}"))?;
            if uri.scheme().is_none()
                || uri.authority().is_none()
                || uri
                    .path_and_query()
                    .is_some_and(|path| path.as_str() != "/")
            {
                return Err(format!(
                    "invalid CORS origin '{origin}': expected scheme://host[:port] with no path"
                ));
            }
            let value = HeaderValue::from_str(origin)
                .map_err(|err| format!("invalid CORS origin '{origin}': {err}"))?;
            if !allowed_origins.contains(&value) {
                allowed_origins.push(value);
            }
        }
        Ok(Self { allowed_origins })
    }

    pub fn allowed_origin_count(&self) -> usize {
        self.allowed_origins.len()
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::new(
            DEFAULT_CORS_ALLOWED_ORIGINS
                .split(',')
                .map(str::to_string)
                .collect(),
        )
        .expect("built-in CORS origins are valid")
    }
}
