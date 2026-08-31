use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;

use runinator_plugin::provider::Provider;

use crate::{
    ConsoleProvider,
    params::{parse_input_params, parse_params},
};

fn request(parameters: runinator_models::value::Value) -> ProviderExecutionRequest {
    ProviderExecutionRequest {
        run_id: None,
        action_name: "console".into(),
        action_function: "run".into(),
        parameters,
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
    }
}

#[test]
fn parse_params_accepts_command_string() {
    let params = parse_params(&request(json!({ "command": "printf hello" }))).unwrap();

    assert_eq!(params.command, "printf hello");
    // interactive defaults to false when the field is omitted.
    assert!(!params.interactive);
}

#[test]
fn parse_params_reads_interactive_flag() {
    let params = parse_params(&request(
        json!({ "command": "aws sso login", "interactive": true }),
    ))
    .unwrap();

    assert!(params.interactive);
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

#[test]
fn parse_input_params_requires_a_prompt() {
    let params = parse_input_params(&request(json!({ "prompt": "Continue?" }))).unwrap();
    assert_eq!(params.prompt, "Continue?");

    assert!(parse_input_params(&request(json!({}))).is_err());
}

#[test]
fn metadata_advertises_the_typed_input_function() {
    let metadata = ConsoleProvider.metadata();
    let input = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "input")
        .expect("console.input metadata");

    assert_eq!(input.parameters.len(), 1);
    assert_eq!(input.parameters[0].name, "prompt");
    assert!(input.parameters[0].required);
    assert_eq!(input.results.len(), 1);
    assert_eq!(input.results[0].name, "value");
}
