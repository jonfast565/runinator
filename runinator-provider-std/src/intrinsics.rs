use std::time::Duration;

use runinator_models::value::{Map, Value};
use runinator_plugin::cancel::CancellationToken;
use runinator_workflows::{
    EFFECTFUL_INTRINSIC_NAMES, IntrinsicLibrary, PureIntrinsics, WorkflowValidationError, call_pure,
};

/// the worker-side superset library: it delegates pure names to the shared pure intrinsics and adds
/// effectful operations (http, time, identifiers, environment). it carries an HTTP client bounded
/// by the action timeout and a cancellation token checked before each effectful call.
pub struct FullIntrinsics {
    client: reqwest::blocking::Client,
    token: CancellationToken,
}

impl FullIntrinsics {
    /// build the library with an HTTP client bounded by `timeout_secs` and the cancellation token.
    pub fn new(timeout_secs: i64, token: CancellationToken) -> Self {
        let timeout = Duration::from_secs(timeout_secs.clamp(1, 3600) as u64);
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();
        Self { client, token }
    }
}

impl IntrinsicLibrary for FullIntrinsics {
    fn call(&self, name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError> {
        if PureIntrinsics::contains(name) {
            return call_pure(name, args);
        }
        // effectful ops honor cancellation: bail before issuing a side effect.
        if self.token.is_cancelled() {
            return Err(WorkflowValidationError::IntrinsicError {
                name: name.to_string(),
                message: "canceled".into(),
            });
        }
        match name {
            "now" => Ok(Value::String(chrono::Utc::now().to_rfc3339())),
            "uuid" => Ok(Value::String(uuid::Uuid::new_v4().to_string())),
            "env" => env(args),
            "http_get" => self.http_get(args),
            "http_post" => self.http_post(args),
            _ => Err(WorkflowValidationError::UnknownIntrinsic(name.to_string())),
        }
    }

    fn knows(&self, name: &str) -> bool {
        PureIntrinsics::contains(name) || EFFECTFUL_INTRINSIC_NAMES.contains(&name)
    }

    fn is_pure(&self, name: &str) -> bool {
        PureIntrinsics::contains(name)
    }
}

fn string_arg(name: &str, args: &[Value], index: usize) -> Result<String, WorkflowValidationError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: name.to_string(),
            message: format!("expected a string argument at position {index}"),
        })
}

impl FullIntrinsics {
    fn http_get(&self, args: &[Value]) -> Result<Value, WorkflowValidationError> {
        let url = string_arg("http_get", args, 0)?;
        let response = self.client.get(&url).send().map_err(http_err)?;
        build_response(response)
    }

    fn http_post(&self, args: &[Value]) -> Result<Value, WorkflowValidationError> {
        let url = string_arg("http_post", args, 0)?;
        let mut request = self.client.post(&url);
        if let Some(body) = args.get(1) {
            request = request.json(&serde_json::Value::from(body.clone()));
        }
        let response = request.send().map_err(http_err)?;
        build_response(response)
    }
}

fn env(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let name = string_arg("env", args, 0)?;
    Ok(std::env::var(&name)
        .map(Value::String)
        .unwrap_or(Value::Null))
}

fn build_response(response: reqwest::blocking::Response) -> Result<Value, WorkflowValidationError> {
    let status = response.status().as_u16() as i64;
    let text = response.text().map_err(http_err)?;
    // parse the body as json when possible; otherwise return the raw string.
    let body = serde_json::from_str::<serde_json::Value>(&text)
        .map(Value::from)
        .unwrap_or(Value::String(text));
    Ok(Value::Object(Map::from_iter([
        ("status".into(), Value::from(status)),
        ("body".into(), body),
    ])))
}

// http errors carry the `http` name so the provider can map them to a dedicated STD code.
fn http_err(err: reqwest::Error) -> WorkflowValidationError {
    WorkflowValidationError::IntrinsicError {
        name: "http".into(),
        message: err.to_string(),
    }
}
