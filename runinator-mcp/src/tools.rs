use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowDefinition;

pub(crate) fn tools_from_workflows(workflows: Vec<WorkflowDefinition>) -> Vec<Value> {
    workflows
        .into_iter()
        .filter(|wf| wf.enabled)
        .filter_map(|wf| {
            let id = wf.id?;
            Some(json!({
                "name": tool_name(&wf, id),
                "description": format!("Execute workflow: {}", wf.name),
                "inputSchema": wf.input_type,
            }))
        })
        .collect()
}

pub(crate) fn fixed_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "runinator_list_providers",
            "description": "List provider and action metadata for workflow authoring.",
            "inputSchema": object_schema(vec![], vec![]),
        }),
        json!({
            "name": "runinator_list_workflows",
            "description": "List saved Runinator workflow definitions.",
            "inputSchema": object_schema(vec![], vec![]),
        }),
        json!({
            "name": "runinator_get_workflow",
            "description": "Fetch a saved Runinator workflow definition.",
            "inputSchema": object_schema(
                vec![("workflow_id", json!({ "type": "integer" }))],
                vec!["workflow_id"],
            ),
        }),
        json!({
            "name": "runinator_validate_workflow",
            "description": "Normalize and validate a Runinator workflow definition without saving it.",
            "inputSchema": object_schema(
                vec![("workflow", json!({ "type": "object" }))],
                vec!["workflow"],
            ),
        }),
        json!({
            "name": "runinator_save_workflow",
            "description": "Create or update a Runinator workflow definition.",
            "inputSchema": object_schema(
                vec![("workflow", json!({ "type": "object" }))],
                vec!["workflow"],
            ),
        }),
        json!({
            "name": "runinator_import_workflow_bundle",
            "description": "Import an importer-compatible workflow bundle.",
            "inputSchema": object_schema(
                vec![
                    ("workflows", json!({ "type": "array", "items": { "type": "object" } })),
                    ("triggers", json!({ "type": "array", "items": { "type": "object" } })),
                ],
                vec![],
            ),
        }),
        json!({
            "name": "runinator_export_workflow_bundle",
            "description": "Export all workflows, or one workflow, as importer-compatible JSON.",
            "inputSchema": object_schema(
                vec![("workflow_id", json!({ "type": "integer" }))],
                vec![],
            ),
        }),
    ]
}

fn object_schema(properties: Vec<(&str, Value)>, required: Vec<&str>) -> Value {
    let mut property_map = Map::new();
    for (name, schema) in properties {
        property_map.insert(name.into(), schema);
    }
    json!({
        "type": "object",
        "properties": property_map,
        "required": required,
    })
}

pub(crate) fn parse_tool_workflow_id(name: &str) -> Option<i64> {
    name.split('_').next_back()?.parse().ok()
}

fn tool_name(wf: &WorkflowDefinition, id: i64) -> String {
    let slug = wf
        .name
        .chars()
        .map(|ch: char| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    format!("{slug}_{id}")
}
