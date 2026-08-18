//! covers exposing saved workflows as tools: which ones become tools, the name that carries the id
//! back, and the schema their declared input turns into.

use super::*;

fn workflow(source: Value) -> WorkflowDefinition {
    serde_json::from_value(source.into()).expect("a workflow definition")
}

fn simple(name: &str, id: &str, enabled: bool) -> WorkflowDefinition {
    workflow(json!({ "id": id, "name": name, "enabled": enabled }))
}

const ID: &str = "8f14e45f-ceea-467a-9a2c-8d1e4d1c9b21";

#[test]
fn an_enabled_workflow_becomes_a_tool_whose_name_carries_its_id() {
    let tools = definitions(vec![simple("Deploy Service", ID, true)]);
    let name = tools[0]
        .get("name")
        .and_then(Value::as_str)
        .expect("a tool name");
    assert!(name.starts_with("deploy_service_"), "{name}");
    assert_eq!(workflow_id_for(name), Some(ID.parse().unwrap()));
}

// a disabled workflow cannot be run, so offering it as a tool would only produce a failed call.
#[test]
fn a_disabled_workflow_is_not_a_tool() {
    assert!(definitions(vec![simple("Deploy", ID, false)]).is_empty());
}

// an unsaved definition has no id to start a run of.
#[test]
fn a_workflow_without_an_id_is_not_a_tool() {
    let unsaved = workflow(json!({ "id": Value::Null, "name": "Draft", "enabled": true }));
    assert!(definitions(vec![unsaved]).is_empty());
}

#[test]
fn a_name_that_is_all_punctuation_still_yields_the_id() {
    let tools = definitions(vec![simple("!!!", ID, true)]);
    let name = tools[0].get("name").and_then(Value::as_str).unwrap();
    assert_eq!(workflow_id_for(name), Some(ID.parse().unwrap()));
}

#[test]
fn a_command_tool_name_does_not_read_as_a_workflow() {
    assert_eq!(workflow_id_for("runinator_workflows_apply"), None);
    assert_eq!(workflow_id_for("runinator_exec"), None);
}

#[test]
fn the_description_names_the_workflow_and_its_version() {
    let described = workflow(json!({
        "id": ID,
        "name": "Deploy",
        "namespace": "platform",
        "version": "2.1.0",
        "enabled": true,
    }));
    let tools = definitions(vec![described]);
    let text = tools[0].get("description").and_then(Value::as_str).unwrap();
    assert!(text.contains("platform.Deploy"), "{text}");
    assert!(text.contains("2.1.0"), "{text}");
}

// the declared input is the schema: a workflow that says what it takes should not be called with a
// free-form object.
#[test]
fn a_declared_input_becomes_the_tool_schema() {
    let typed = workflow(json!({
        "id": ID,
        "name": "Deploy",
        "enabled": true,
        "input_type": {
            "type": "struct",
            "fields": {
                "region": { "ty": { "type": "string" }, "required": true },
                "replicas": { "ty": { "type": "integer" }, "required": false },
            },
        },
    }));
    let tools = definitions(vec![typed]);
    let schema = tools[0].get("inputSchema").expect("a schema");
    assert_eq!(
        schema
            .pointer("/properties/region/type")
            .and_then(Value::as_str),
        Some("string")
    );
    assert_eq!(
        schema
            .pointer("/properties/replicas/type")
            .and_then(Value::as_str),
        Some("integer")
    );
    assert_eq!(
        schema.get("required").and_then(Value::as_array),
        Some(&vec![Value::from("region")])
    );
}

// `Any` is the absence of a constraint, and the protocol needs an object at the top level anyway.
#[test]
fn an_untyped_input_becomes_an_open_object() {
    let tools = definitions(vec![simple("Deploy", ID, true)]);
    assert_eq!(
        tools[0]
            .pointer("/inputSchema/type")
            .and_then(Value::as_str),
        Some("object")
    );
}

#[test]
fn scalars_and_collections_map_to_their_json_types() {
    assert_eq!(
        type_schema(&RuninatorType::Boolean),
        json!({ "type": "boolean" })
    );
    assert_eq!(
        type_schema(&RuninatorType::Array(Box::new(RuninatorType::String))),
        json!({ "type": "array", "items": { "type": "string" } })
    );
    assert_eq!(
        type_schema(&RuninatorType::Map(Box::new(RuninatorType::Integer))),
        json!({ "type": "object", "additionalProperties": { "type": "integer" } })
    );
    // a lambda cannot be written as json, and an empty schema is what json schema calls "anything".
    assert_eq!(type_schema(&RuninatorType::Any), json!({}));
}

#[test]
fn a_range_carries_its_bounds() {
    let ranged = RuninatorType::Range {
        base: Box::new(RuninatorType::Integer),
        min: Some(Value::from(1)),
        max: Some(Value::from(10)),
    };
    assert_eq!(
        type_schema(&ranged),
        json!({ "type": "integer", "minimum": 1, "maximum": 10 })
    );
}

#[test]
fn an_enum_is_its_values() {
    let choices = RuninatorType::Enum(vec![Value::from("dev"), Value::from("prod")]);
    assert_eq!(type_schema(&choices), json!({ "enum": ["dev", "prod"] }));
}

// a field with a default is satisfiable without being given, so it is not required of the caller
// even when the type says it is required of the run.
#[test]
fn a_defaulted_field_is_not_required_of_the_caller() {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "region".to_string(),
        RuninatorField {
            ty: RuninatorType::String,
            required: true,
            default: Some(Value::from("us-east-1")),
        },
    );
    let schema = struct_schema(&fields, None);
    assert_eq!(
        schema.get("required").and_then(Value::as_array),
        Some(&Vec::new())
    );
    assert_eq!(
        schema
            .pointer("/properties/region/default")
            .and_then(Value::as_str),
        Some("us-east-1")
    );
}

// a lowered default may be an expression that means nothing to a client, which reads a `default` as
// a value it may send back.
#[test]
fn an_expression_default_is_not_offered_as_a_value() {
    assert!(is_expression(&json!({ "$ref": "input.region" })));
    assert!(!is_expression(&json!({ "region": "us-east-1" })));

    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "region".to_string(),
        RuninatorField {
            ty: RuninatorType::String,
            required: false,
            default: Some(json!({ "$ref": "input.region" })),
        },
    );
    let schema = struct_schema(&fields, None);
    assert!(schema.pointer("/properties/region/default").is_none());
}
