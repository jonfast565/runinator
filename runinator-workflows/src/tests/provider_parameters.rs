//! typing an action node against its provider metadata: required parameters, literal struct fields,
//! and errors raised from inside a nested dynamic configuration.

use super::*;

#[test]
fn typed_validation_rejects_provider_default_value_mismatch() {
    let provider = ProviderMetadata {
        name: "typed".into(),
        actions: vec![
            ActionMetadata::new("check", "check typed input").with_parameters(vec![
                ParameterMetadata::optional("count", RuninatorType::Integer)
                    .with_default(runinator_models::json!("bad")),
            ]),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    };
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(
        err.to_string()
            .contains("provider 'typed.check' parameter 'count' expected integer, got string")
    );
}

#[test]
fn typed_validation_rejects_whitespace_only_required_parameter() {
    let wf = action_workflow(runinator_models::json!({ "config": "   " }));
    let err = validate_workflow_with_providers(&wf, &[check_provider(RuninatorType::String)])
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("missing required action parameter 'config'")
    );
}

#[test]
fn typed_validation_accepts_non_blank_required_parameter() {
    let wf = action_workflow(runinator_models::json!({ "config": "value" }));
    validate_workflow_with_providers(&wf, &[check_provider(RuninatorType::String)])
        .expect("non-blank required parameter is accepted");
}

#[test]
fn typed_validation_reports_missing_required_nested_literal_field() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "env",
        RuninatorField::required(RuninatorType::typed_structure([(
            "API_KEY",
            RuninatorField::required(RuninatorType::String),
        )])),
    )]));
    let wf = action_workflow(runinator_models::json!({
        "config": { "env": {} }
    }));

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(
        err.to_string()
            .contains("action parameter 'config.env.API_KEY' is missing required field")
    );
    let diagnostic = err
        .type_diagnostic()
        .expect("type diagnostic is structured");
    assert_eq!(diagnostic.path, "action parameter 'config.env.API_KEY'");
    assert_eq!(diagnostic.expected, "string");
    assert_eq!(diagnostic.actual, "missing");
}

#[test]
fn typed_validation_accepts_absent_optional_literal_field() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "env",
        RuninatorField::optional(RuninatorType::typed_structure([(
            "API_KEY",
            RuninatorField::required(RuninatorType::String),
        )])),
    )]));
    let wf = action_workflow(runinator_models::json!({ "config": {} }));

    validate_workflow_with_providers(&wf, &[provider]).expect("optional field may be absent");
}

#[test]
fn typed_validation_rejects_closed_struct_additional_literal_fields() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "name",
        RuninatorField::required(RuninatorType::String),
    )]));
    let wf = action_workflow(runinator_models::json!({
        "config": { "name": "build", "extra": true }
    }));

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(
        err.to_string()
            .contains("action parameter 'config.extra' is not allowed")
    );
}

#[test]
fn typed_validation_validates_open_struct_additional_literal_fields() {
    let provider = check_provider(RuninatorType::open_typed_structure(
        [("name", RuninatorField::required(RuninatorType::String))],
        RuninatorType::String,
    ));
    let valid = action_workflow(runinator_models::json!({
        "config": { "name": "build", "extra": "ok" }
    }));
    validate_workflow_with_providers(&valid, std::slice::from_ref(&provider))
        .expect("open struct validates additional field values");

    let invalid = action_workflow(runinator_models::json!({
        "config": { "name": "build", "extra": 1 }
    }));
    let err = validate_workflow_with_providers(&invalid, &[provider]).unwrap_err();
    assert!(
        err.to_string()
            .contains("action parameter 'config.extra' expected string, got integer")
    );
}

#[test]
fn typed_validation_reports_nested_literal_errors_inside_dynamic_configs() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "env",
        RuninatorField::required(RuninatorType::typed_structure([
            ("API_KEY", RuninatorField::required(RuninatorType::String)),
            ("TOKEN", RuninatorField::required(RuninatorType::String)),
        ])),
    )]));
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "env": {
                "API_KEY": 1,
                "TOKEN": { "$ref": { "params": ["token"] } }
            }
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "token",
        RuninatorField::required(RuninatorType::String),
    )]);

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(
        err.to_string()
            .contains("action parameter 'config.env.API_KEY' expected string, got integer")
    );
}

#[test]
fn typed_validation_reports_nested_dynamic_expression_type_errors() {
    let provider = check_provider(RuninatorType::typed_structure([(
        "branch",
        RuninatorField::required(RuninatorType::String),
    )]));
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "branch": { "$ref": { "params": ["count"] } }
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "count",
        RuninatorField::required(RuninatorType::Integer),
    )]);

    let err = validate_workflow_with_providers(&wf, &[provider]).unwrap_err();
    assert!(
        err.to_string()
            .contains("action parameter 'config.branch' expected string, got integer")
    );
}

// an open `{ scope: { name: type } }` config schema, matching what the web service builds from
// the stored settings.
fn config_schema(name_type: RuninatorType) -> RuninatorType {
    RuninatorType::open_structure(
        [(
            "jira",
            RuninatorType::open_structure([("base_url", name_type)], RuninatorType::Any),
        )],
        RuninatorType::Any,
    )
}

#[test]
fn typed_validation_accepts_config_ref_matching_config_schema() {
    let provider = check_provider(RuninatorType::String);
    let wf = action_workflow(runinator_models::json!({
        "config": { "$ref": { "config": ["jira", "base_url"] } }
    }));
    validate_workflow_with_config(&wf, &[provider], &config_schema(RuninatorType::String))
        .expect("config ref typed as string satisfies a string parameter");
}

#[test]
fn typed_validation_rejects_config_ref_conflicting_with_config_schema() {
    let provider = check_provider(RuninatorType::String);
    let wf = action_workflow(runinator_models::json!({
        "config": { "$ref": { "config": ["jira", "base_url"] } }
    }));
    let err =
        validate_workflow_with_config(&wf, &[provider], &config_schema(RuninatorType::Integer))
            .unwrap_err();
    assert!(
        err.to_string()
            .contains("action parameter 'config' expected string, got integer"),
        "unexpected error: {err}"
    );
}

#[test]
fn typed_validation_types_unconfigured_config_refs_as_any() {
    // a key absent from the schema falls through the open struct's `any` additional type, so it
    // resolves to `any` (assignable to an `any` parameter) rather than erroring on the ref itself.
    let provider = check_provider(RuninatorType::Any);
    let wf = action_workflow(runinator_models::json!({
        "config": { "$ref": { "config": ["jira", "not_configured"] } }
    }));
    validate_workflow_with_config(&wf, &[provider], &config_schema(RuninatorType::String))
        .expect("unconfigured config keys resolve to any");
}

#[test]
fn typed_validation_keeps_optional_field_refs_presence_only() {
    let provider = check_provider(RuninatorType::String);
    let mut wf = action_workflow(runinator_models::json!({
        "config": { "$ref": { "params": ["maybe_name"] } }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "maybe_name",
        RuninatorField::optional(RuninatorType::String),
    )]);

    validate_workflow_with_providers(&wf, &[provider])
        .expect("optional refs resolve as their declared type");
}

#[test]
fn typed_validation_accepts_explicit_coalesce_defaults() {
    let provider = check_provider(RuninatorType::String);
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "$coalesce": [
                { "$ref": { "params": ["maybe_name"] } },
                "fallback"
            ]
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "maybe_name",
        RuninatorField::optional(RuninatorType::String),
    )]);

    validate_workflow_with_providers(&wf, &[provider])
        .expect("coalesce resolves to the fallback-compatible type");
}
