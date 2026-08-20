//! exposing saved workflows as callable tools.
//!
//! this is the other direction from the rest of the server: not "let a model author runinator" but
//! "let a model call what runinator already runs". a workflow's own `input_type` is its tool schema,
//! so the model sees the declared parameters rather than a free-form object.
//!
//! it is off by default. the tool list is context the model pays for on every turn, and a fleet of
//! workflows would bury the two tools that author them under a hundred that call them.

use std::time::Duration;

use runinator_models::json;
use runinator_models::types::{RuninatorField, RuninatorType};
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowDefinition, WorkflowStatus};
use uuid::Uuid;

use super::protocol::{structured_result, text_result};
use crate::commands::Client;

/// how long a call waits for the run to settle before handing back the run id.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

/// how often the run is re-read while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// one tool per enabled workflow that has been saved.
pub(crate) fn definitions(workflows: Vec<WorkflowDefinition>) -> Vec<Value> {
    workflows
        .into_iter()
        .filter(|workflow| workflow.enabled)
        .filter_map(|workflow| {
            let id = workflow.id?;
            Some(json!({
                "name": tool_name(&workflow, id),
                "description": format!(
                    "Start a run of the Runinator workflow \"{}\" (version {}) and return what it \
                     did. A run that settles quickly answers inside the call; a longer one hands \
                     back its run id to follow with `runinator_runs_show`.",
                    qualified_name(&workflow),
                    workflow.version,
                ),
                "inputSchema": input_schema(&workflow.input_type),
                "annotations": {
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "idempotentHint": false,
                    "openWorldHint": true,
                },
            }))
        })
        .collect()
}

/// the workflow id a generated tool name carries, if it carries one.
pub(crate) fn workflow_id_for(name: &str) -> Option<Uuid> {
    name.split('_').next_back()?.parse().ok()
}

/// start a run of the named workflow and wait briefly for it to settle.
pub(crate) async fn call(client: &Client, name: &str, arguments: Value) -> Value {
    let Some(workflow_id) = workflow_id_for(name) else {
        return text_result(format!("tool '{name}' does not name a workflow"), true);
    };
    let run = match client.create_workflow_run(workflow_id, arguments).await {
        Ok(run) => run,
        Err(failure) => return text_result(failure.to_string(), true),
    };

    // a short workflow answering inside the call is worth waiting for; a long one hands back its id
    // so the caller can follow it with `runs show`.
    let settled = wait_for_settle(client, run.id).await;
    let (status, payload) = match settled {
        Some((status, payload)) => (status, payload),
        None => (run.status, json!({ "run": run })),
    };
    if !status.is_terminal() {
        return structured_result(
            format!(
                "workflow run {} is {}. follow it with `runs show {}`.",
                run.id,
                status.as_str(),
                run.id
            ),
            payload,
            false,
        );
    }
    structured_result(
        format!("workflow run {} finished {}.", run.id, status.as_str()),
        payload,
        status != WorkflowStatus::Succeeded,
    )
}

async fn wait_for_settle(client: &Client, run_id: Uuid) -> Option<(WorkflowStatus, Value)> {
    let deadline = tokio::time::Instant::now() + SETTLE_TIMEOUT;
    loop {
        let (run, nodes) = client.fetch_workflow_run(run_id).await.ok()?;
        let status = run.status;
        let payload = json!({ "run": run, "nodes": nodes });
        if status.is_terminal() || tokio::time::Instant::now() >= deadline {
            return Some((status, payload));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// a workflow's tool name: its name reduced to identifier characters, then its id.
///
/// the id is the payload — it is what `workflow_id_for` reads back — and the slug is only there so
/// the model can tell two tools apart in a list.
fn tool_name(workflow: &WorkflowDefinition, id: Uuid) -> String {
    let slug = workflow
        .name
        .chars()
        .map(|character| match character.is_ascii_alphanumeric() {
            true => character.to_ascii_lowercase(),
            false => '_',
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    format!("{slug}_{id}")
}

/// a workflow's declared input, as the json schema a client validates a call against.
///
/// the declared type is the schema: a workflow that says what it takes should not be called with a
/// free-form object, which is what the model would otherwise be handed. `Any` is the one case with
/// nothing to say, and an open object is the honest rendering of it.
fn input_schema(input: &RuninatorType) -> Value {
    match input {
        RuninatorType::Struct { .. } => type_schema(input),
        // the protocol requires an object at the top level, so anything else is not a shape a tool
        // call can carry.
        _ => json!({ "type": "object" }),
    }
}

fn type_schema(input: &RuninatorType) -> Value {
    match input {
        RuninatorType::Null => json!({ "type": "null" }),
        RuninatorType::Boolean => json!({ "type": "boolean" }),
        RuninatorType::Integer => json!({ "type": "integer" }),
        RuninatorType::Number => json!({ "type": "number" }),
        // a duration is written the way REXRAP writes it, so the format is the useful part.
        RuninatorType::Duration => {
            json!({ "type": "string", "description": "a duration, e.g. \"30s\", \"5m\", \"2h\"" })
        }
        RuninatorType::String => json!({ "type": "string" }),
        RuninatorType::Enum(values) => json!({ "enum": values.clone() }),
        RuninatorType::Range { base, min, max } => range_schema(base, min.as_ref(), max.as_ref()),
        RuninatorType::Array(item) => json!({ "type": "array", "items": type_schema(item) }),
        RuninatorType::Map(value) => {
            json!({ "type": "object", "additionalProperties": type_schema(value) })
        }
        RuninatorType::Struct { fields, additional } => {
            struct_schema(fields, additional.as_deref())
        }
        RuninatorType::Union(variants) => json!({
            "anyOf": variants.iter().map(type_schema).collect::<Vec<_>>(),
        }),
        // a lambda cannot be written as json, and `Any` is the absence of a constraint. neither has
        // a schema, and an empty one is what json schema calls "anything".
        RuninatorType::Function { .. } | RuninatorType::Any => json!({}),
    }
}

fn range_schema(base: &RuninatorType, min: Option<&Value>, max: Option<&Value>) -> Value {
    let mut schema = match type_schema(base) {
        Value::Object(map) => map,
        _ => runinator_models::value::Map::new(),
    };
    if let Some(min) = min {
        schema.insert("minimum".into(), min.clone());
    }
    if let Some(max) = max {
        schema.insert("maximum".into(), max.clone());
    }
    Value::Object(schema)
}

fn struct_schema(
    fields: &std::collections::BTreeMap<String, RuninatorField>,
    additional: Option<&RuninatorType>,
) -> Value {
    let mut properties = runinator_models::value::Map::new();
    let mut required = Vec::new();
    for (name, field) in fields {
        let mut schema = match type_schema(&field.ty) {
            Value::Object(map) => map,
            other => {
                properties.insert(name.clone(), other);
                continue;
            }
        };
        // a lowered default is an expression, not a value; only a literal is worth showing.
        if let Some(default) = field.default.as_ref().filter(|value| !is_expression(value)) {
            schema.insert("default".into(), default.clone());
        }
        properties.insert(name.clone(), Value::Object(schema));
        // a field with a default is satisfiable without being given, so it is not required of the
        // caller even when the type says it is required of the run.
        if field.required && field.default.is_none() {
            required.push(Value::from(name.clone()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": match additional {
            Some(ty) => type_schema(ty),
            None => Value::Bool(false),
        },
    })
}

// a lowered default may be an expression object (`{"$ref": …}`, `{"$concat": …}`) that means
// nothing to a client, which reads a `default` as a value it may send back.
fn is_expression(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.keys().any(|key| key.starts_with('$')))
}

/// the workflow's name as its namespace qualifies it, which is how a subflow target names it too.
fn qualified_name(workflow: &WorkflowDefinition) -> String {
    match workflow.namespace.as_deref().filter(|ns| !ns.is_empty()) {
        Some(namespace) => format!("{namespace}.{}", workflow.name),
        None => workflow.name.clone(),
    }
}

#[cfg(test)]
#[path = "workflow_tools_tests.rs"]
mod tests;
