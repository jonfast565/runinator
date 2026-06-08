use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::Provider;
use uuid::Uuid;

use crate::StdProvider;

fn request(parameters: runinator_models::value::Value) -> ProviderExecutionRequest {
    ProviderExecutionRequest {
        run_id: Some(Uuid::now_v7()),
        action_name: "std".into(),
        action_function: "exec".into(),
        parameters,
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
    }
}

#[test]
fn exec_program_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "program": [
            { "$let": "n", "value": { "$add": [{ "$ref": { "input": ["a"] } }, 1] } },
            { "$return": { "n": { "$ref": { "let": ["n"] } } } }
        ],
        "context": { "input": { "a": 41 } }
    });
    let result = provider
        .execute_service(request(parameters), None, CancellationToken::new())
        .unwrap();
    assert_eq!(result.output_json, Some(json!({ "n": 42 })));
}

#[test]
fn exec_uses_effectful_intrinsic() {
    let provider = StdProvider;
    let parameters = json!({
        "program": [ { "$return": { "$call": "uuid", "args": [] } } ],
        "context": {}
    });
    let result = provider
        .execute_service(request(parameters), None, CancellationToken::new())
        .unwrap();
    // a uuid string of canonical length is produced by the effectful library.
    let value = result.output_json.unwrap();
    assert_eq!(value.as_str().map(str::len), Some(36));
}

#[test]
fn metadata_advertises_run_exec_and_pure_flags() {
    let metadata = StdProvider.metadata();
    let run = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "run")
        .unwrap();
    assert!(run.pure);
    let exec = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "exec")
        .unwrap();
    assert!(!exec.pure);
    let add = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "add")
        .unwrap();
    assert!(add.pure);
    let http = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "http_get")
        .unwrap();
    assert!(!http.pure);
}
