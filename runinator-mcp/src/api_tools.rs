use std::collections::{BTreeMap, BTreeSet};

use runinator_models::json;
use runinator_models::value::{Map, Value};

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];
const COMPONENT_SCHEMA_PREFIX: &str = "#/components/schemas/";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApiParameter {
    pub(crate) name: String,
    pub(crate) location: String,
    pub(crate) required: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ApiTool {
    pub(crate) name: String,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) parameters: Vec<ApiParameter>,
    pub(crate) request_content_type: Option<String>,
    pub(crate) request_required: bool,
    definition: Value,
}

impl ApiTool {
    pub(crate) fn definition(&self) -> Value {
        self.definition.clone()
    }
}

/// build one mcp tool for every request-response operation in the service's openapi document.
pub(crate) fn api_tools_from_openapi(document: &Value) -> Vec<ApiTool> {
    let Some(paths) = document.get("paths").and_then(Value::as_object) else {
        return Vec::new();
    };
    let component_schemas = document
        .pointer("/components/schemas")
        .and_then(Value::as_object);
    let mut tools = Vec::new();

    for (path, path_item) in paths {
        let Some(path_item) = path_item.as_object() else {
            continue;
        };
        for method in HTTP_METHODS {
            let Some(operation) = path_item.get(method).and_then(Value::as_object) else {
                continue;
            };
            // websocket upgrades cannot be represented by a single mcp tool result.
            if operation
                .get("responses")
                .and_then(|responses| responses.get("101"))
                .is_some()
            {
                continue;
            }
            tools.push(api_tool(method, path, operation, component_schemas));
        }
    }

    tools.sort_by(|left, right| left.name.cmp(&right.name));
    tools
}

fn api_tool(method: &str, path: &str, operation: &Map, component_schemas: Option<&Map>) -> ApiTool {
    let parameters = operation
        .get("parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_parameter)
        .collect::<Vec<_>>();
    let (request_content_type, request_required, body_schema) = request_body(operation);
    let input_schema = input_schema(operation, &parameters, body_schema, component_schemas);
    let name = api_tool_name(method, path);
    let summary = operation
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("Call a Runinator API operation");
    let description = operation
        .get("description")
        .and_then(Value::as_str)
        .filter(|description| !description.trim().is_empty())
        .map(|description| {
            format!(
                "{summary}\n\n{description}\n\nHTTP {} {path}",
                method.to_uppercase()
            )
        })
        .unwrap_or_else(|| format!("{summary}\n\nHTTP {} {path}", method.to_uppercase()));
    let read_only = method == "get";
    let destructive = method == "delete";
    let definition = json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": matches!(method, "get" | "put" | "delete"),
            "openWorldHint": false,
        },
    });

    ApiTool {
        name,
        method: method.to_string(),
        path: path.to_string(),
        parameters,
        request_content_type,
        request_required,
        definition,
    }
}

fn parse_parameter(parameter: &Value) -> Option<ApiParameter> {
    Some(ApiParameter {
        name: parameter.get("name")?.as_str()?.to_string(),
        location: parameter.get("in")?.as_str()?.to_string(),
        required: parameter
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn request_body(operation: &Map) -> (Option<String>, bool, Option<Value>) {
    let Some(request_body) = operation.get("requestBody") else {
        return (None, false, None);
    };
    let required = request_body
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(content) = request_body.get("content").and_then(Value::as_object) else {
        return (None, required, Some(json!({ "type": "object" })));
    };
    let selected = content
        .get("application/json")
        .map(|media| ("application/json", media))
        .or_else(|| {
            content
                .iter()
                .next()
                .map(|(name, media)| (name.as_str(), media))
        });
    let Some((content_type, media)) = selected else {
        return (None, required, Some(json!({ "type": "object" })));
    };
    let schema = media
        .get("schema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object" }));
    (Some(content_type.to_string()), required, Some(schema))
}

fn input_schema(
    operation: &Map,
    parameters: &[ApiParameter],
    body_schema: Option<Value>,
    component_schemas: Option<&Map>,
) -> Value {
    let documented_parameters = operation.get("parameters").and_then(Value::as_array);
    let mut properties = Map::new();
    let mut required = Vec::new();

    for parameter in parameters {
        let documented = documented_parameters
            .into_iter()
            .flatten()
            .find(|candidate| {
                candidate.get("name").and_then(Value::as_str) == Some(parameter.name.as_str())
                    && candidate.get("in").and_then(Value::as_str)
                        == Some(parameter.location.as_str())
            });
        let mut schema = documented
            .and_then(|parameter| parameter.get("schema"))
            .cloned()
            .unwrap_or_else(|| json!({ "type": "string" }));
        if let Some(description) = documented
            .and_then(|parameter| parameter.get("description"))
            .and_then(Value::as_str)
            && let Some(schema) = schema.as_object_mut()
        {
            schema
                .entry("description")
                .or_insert_with(|| json!(description));
        }
        properties.insert(parameter.name.clone(), schema);
        if parameter.required {
            required.push(Value::String(parameter.name.clone()));
        }
    }

    if let Some(body_schema) = body_schema {
        properties.insert("body".into(), body_schema);
        if operation
            .get("requestBody")
            .and_then(|body| body.get("required"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            required.push(Value::String("body".into()));
        }
    }

    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    });
    attach_referenced_definitions(&mut schema, component_schemas);
    schema
}

fn attach_referenced_definitions(schema: &mut Value, component_schemas: Option<&Map>) {
    let Some(component_schemas) = component_schemas else {
        rewrite_component_refs(schema);
        return;
    };
    let mut names = BTreeSet::new();
    collect_references(schema, component_schemas, &mut names);
    rewrite_component_refs(schema);
    if names.is_empty() {
        return;
    }

    let mut definitions = BTreeMap::new();
    for name in names {
        if let Some(component) = component_schemas.get(&name) {
            let mut component = component.clone();
            rewrite_component_refs(&mut component);
            definitions.insert(name, component);
        }
    }
    schema
        .as_object_mut()
        .expect("input schema is an object")
        .insert("$defs".into(), json!(definitions));
}

fn collect_references(value: &Value, component_schemas: &Map, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix(COMPONENT_SCHEMA_PREFIX))
                && names.insert(name.to_string())
                && let Some(component) = component_schemas.get(name)
            {
                collect_references(component, component_schemas, names);
            }
            for child in object.values() {
                collect_references(child, component_schemas, names);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_references(child, component_schemas, names);
            }
        }
        _ => {}
    }
}

fn rewrite_component_refs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get_mut("$ref")
                && let Some(name) = reference
                    .as_str()
                    .and_then(|reference| reference.strip_prefix(COMPONENT_SCHEMA_PREFIX))
            {
                *reference = Value::String(format!("#/$defs/{name}"));
            }
            for child in object.values_mut() {
                rewrite_component_refs(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                rewrite_component_refs(child);
            }
        }
        _ => {}
    }
}

fn api_tool_name(method: &str, path: &str) -> String {
    let suffix = path
        .trim_matches('/')
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!("runinator_api_{method}_{suffix}")
}
