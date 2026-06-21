use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;

use crate::params::parse_params;

fn request(parameters: runinator_models::value::Value) -> ProviderExecutionRequest {
    ProviderExecutionRequest {
        run_id: None,
        action_name: "console".into(),
        action_function: "run".into(),
        parameters,
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
    }
}

#[test]
fn parse_params_accepts_command_string() {
    let params = parse_params(&request(json!({ "command": "printf hello" }))).unwrap();

    assert_eq!(params.command, "printf hello");
}

#[test]
fn parse_params_rejects_missing_command() {
    let err = match parse_params(&request(json!({}))) {
        Ok(_) => panic!("missing command should be rejected"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("CONSOLE001"));
    assert!(err.to_string().contains("missing field `command`"));
}
