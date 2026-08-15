//! covers the provider's contract with the worker: one advertised action, the injected parameters
//! it requires, and how an export's declared limits become the deadline it actually runs under.
//!
//! execution itself is driven through a fake [`InvocationRuntime`] — what belongs to this crate is
//! the request/response shape, and `runinator-sandbox` already tests the container half.

use super::*;

use std::sync::Mutex;

use runinator_models::functions::{FunctionResourceLimits, FunctionRuntimeSpec};
use runinator_models::json;
use runinator_models::value::Value;

use crate::runtime::InvocationOutcome;

#[derive(Default)]
struct FakeRuntime {
    seen: Mutex<Vec<InvocationRequest>>,
    output: Mutex<Value>,
}

impl InvocationRuntime for FakeRuntime {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn invoke(
        &self,
        request: &InvocationRequest,
        _logs: Option<Arc<dyn LineSink>>,
        _token: CancellationToken,
    ) -> Result<InvocationOutcome, SendableError> {
        self.seen.lock().unwrap().push(request.clone());
        Ok(InvocationOutcome {
            output: self.output.lock().unwrap().clone(),
            stdout: String::new(),
            stderr: String::new(),
            truncated: false,
            duration: std::time::Duration::from_millis(10),
        })
    }
}

fn staged() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("runi-fn-pkg-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn execution_request(package: &std::path::Path, parameters: Value) -> ProviderExecutionRequest {
    ProviderExecutionRequest {
        run_id: None,
        action_name: "functions".into(),
        action_function: "invoke".into(),
        parameters,
        timeout_secs: 300,
        artifact_dir: package.to_string_lossy().into(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
    }
}

fn full_parameters(package: &std::path::Path) -> Value {
    json!({
        "package_path": package.to_string_lossy(),
        "handler": "src.images.resize",
        "runtime": { "runtime": "python3.13" },
        "limits": { "timeout_seconds": 45, "memory_mb": 256 },
        "input": { "source": "a.png", "width": 320 },
        "context": { "package": "image-tools", "export": "resize" },
    })
}

#[test]
fn advertises_exactly_one_action() {
    let metadata = FunctionsProvider::default().metadata();
    assert_eq!(metadata.name, "functions");
    // per-export action names would be rejected by the worker's metadata check, and no static list
    // could enumerate every export ever published.
    assert_eq!(metadata.actions.len(), 1);
    assert_eq!(metadata.actions[0].function_name, "invoke");
    assert!(!metadata.actions[0].pure);
}

#[test]
fn rejects_a_function_it_does_not_offer() {
    let package = staged();
    let provider = FunctionsProvider::default();
    let error = provider
        .execute_service(
            ProviderExecutionRequest {
                action_function: "resize".into(),
                ..execution_request(&package, full_parameters(&package))
            },
            None,
            CancellationToken::new(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("FUNC001"));
    let _ = std::fs::remove_dir_all(package);
}

#[test]
fn passes_the_staged_package_and_input_through() {
    let package = staged();
    let runtime = Arc::new(FakeRuntime::default());
    *runtime.output.lock().unwrap() = json!({ "uri": "a.png?w=320" });
    let provider = FunctionsProvider::with_runtime(runtime.clone());

    let result = provider
        .execute_service(
            execution_request(&package, full_parameters(&package)),
            None,
            CancellationToken::new(),
        )
        .unwrap();

    assert_eq!(
        result
            .output_json
            .and_then(|value| value.get("uri").and_then(Value::as_str).map(str::to_string)),
        Some("a.png?w=320".to_string())
    );
    let seen = runtime.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].handler, "src.images.resize");
    assert_eq!(seen[0].package_path, package);
    assert_eq!(
        seen[0].input.get("width").and_then(Value::as_i64),
        Some(320)
    );
    drop(seen);
    let _ = std::fs::remove_dir_all(package);
}

#[test]
fn a_missing_staging_parameter_fails_before_anything_runs() {
    let package = staged();
    let provider = FunctionsProvider::with_runtime(Arc::new(FakeRuntime::default()));
    for missing in ["package_path", "handler", "runtime"] {
        let mut parameters = full_parameters(&package);
        if let Value::Object(object) = &mut parameters {
            object.remove(missing);
        }
        let error = provider
            .execute_service(
                execution_request(&package, parameters),
                None,
                CancellationToken::new(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("FUNC001"), "{missing} should be required");
    }
    let _ = std::fs::remove_dir_all(package);
}

#[test]
fn absent_limits_still_produce_a_bounded_invocation() {
    let package = staged();
    let mut parameters = full_parameters(&package);
    if let Value::Object(object) = &mut parameters {
        object.remove("limits");
    }
    let runtime = Arc::new(FakeRuntime::default());
    let provider = FunctionsProvider::with_runtime(runtime.clone());
    provider
        .execute_service(
            execution_request(&package, parameters),
            None,
            CancellationToken::new(),
        )
        .unwrap();

    // an omitted limit means "the default", never "unlimited".
    let seen = runtime.seen.lock().unwrap();
    assert_eq!(seen[0].limits, FunctionResourceLimits::default());
    assert!(!seen[0].limits.network);
    drop(seen);
    let _ = std::fs::remove_dir_all(package);
}

#[test]
fn the_deadline_is_the_smaller_of_the_export_and_the_node() {
    let package = staged();
    let request = |declared: i64, node: i64| InvocationRequest {
        package_path: package.clone(),
        handler: "h".into(),
        runtime: FunctionRuntimeSpec::new("python3.13"),
        limits: FunctionResourceLimits {
            timeout_seconds: declared,
            ..FunctionResourceLimits::default()
        },
        input: Value::Null,
        context: Value::Null,
        timeout_secs: node,
    };

    // the manifest cannot buy itself more time than the workflow allowed...
    assert_eq!(request(600, 30).effective_timeout_secs(), 30);
    // ...and the workflow cannot make an export run longer than its author said it should.
    assert_eq!(request(45, 300).effective_timeout_secs(), 45);
    // a node with no timeout leaves the export's own limit in force.
    assert_eq!(request(45, 0).effective_timeout_secs(), 45);
    let _ = std::fs::remove_dir_all(package);
}
