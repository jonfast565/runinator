use crate::contracts::{
    RESOURCE_ARTIFACT_URI_PREFIX, RESOURCE_RUN_ARTIFACTS_TEMPLATE_URI,
    RESOURCE_RUN_CHUNKS_TEMPLATE_URI, RESOURCE_RUN_TEMPLATE_URI, RESOURCE_RUN_URI_PREFIX,
    RESOURCE_WORKFLOW_RUN_TEMPLATE_URI, RESOURCE_WORKFLOW_RUN_URI_PREFIX,
    RESOURCE_WORKFLOW_TEMPLATE_URI, RESOURCE_WORKFLOW_URI_PREFIX, RESOURCE_WORKFLOWS_URI,
    STATUS_UNKNOWN,
};
use runinator_models::api_routes::{
    API_ARTIFACTS, API_RUNS, API_WORKFLOWS, api_run, api_run_artifacts, api_workflow,
    api_workflow_run,
};
use runinator_models::json;
use runinator_models::value::Value;

pub(crate) fn resource_templates() -> Vec<Value> {
    vec![
        json!({
            "uri": RESOURCE_WORKFLOWS_URI,
            "name": "Workflow list",
            "mimeType": "application/json"
        }),
        json!({
            "uri": RESOURCE_WORKFLOW_TEMPLATE_URI,
            "name": "Workflow definition",
            "mimeType": "application/json"
        }),
        json!({
            "uri": RESOURCE_WORKFLOW_RUN_TEMPLATE_URI,
            "name": "Workflow run",
            "mimeType": "application/json"
        }),
        json!({
            "uri": RESOURCE_RUN_TEMPLATE_URI,
            "name": "Run summary",
            "mimeType": "application/json"
        }),
        json!({
            "uri": RESOURCE_RUN_CHUNKS_TEMPLATE_URI,
            "name": "Run output chunks",
            "mimeType": "application/json"
        }),
        json!({
            "uri": RESOURCE_RUN_ARTIFACTS_TEMPLATE_URI,
            "name": "Run artifacts",
            "mimeType": "application/json"
        }),
    ]
}

pub(crate) fn resource_path_for_uri(uri: &str) -> Option<String> {
    if uri == RESOURCE_WORKFLOWS_URI {
        return Some(API_WORKFLOWS.trim_start_matches('/').to_string());
    }
    if let Some(workflow_id) = uri
        .strip_prefix(RESOURCE_WORKFLOW_URI_PREFIX)
        .and_then(|id| id.parse::<i64>().ok())
    {
        return Some(api_workflow(workflow_id).trim_start_matches('/').to_string());
    }
    if let Some(workflow_run_id) = uri
        .strip_prefix(RESOURCE_WORKFLOW_RUN_URI_PREFIX)
        .and_then(|id| id.parse::<i64>().ok())
    {
        return Some(api_workflow_run(workflow_run_id).trim_start_matches('/').to_string());
    }
    if let Some(raw) = uri.strip_prefix(RESOURCE_RUN_URI_PREFIX) {
        if let Some(run_id) = raw
            .strip_suffix("/chunks")
            .and_then(|id| id.parse::<i64>().ok())
        {
            return Some(format!(
                "{}/{}{}",
                API_RUNS.trim_start_matches('/'),
                run_id,
                "/chunks?limit=500"
            ));
        }
        if let Some(run_id) = raw
            .strip_suffix("/artifacts")
            .and_then(|id| id.parse::<i64>().ok())
        {
            return Some(api_run_artifacts(run_id).trim_start_matches('/').to_string());
        }
        if let Ok(run_id) = raw.parse::<i64>() {
            return Some(api_run(run_id).trim_start_matches('/').to_string());
        }
    }
    if let Some(artifact_id) = uri
        .strip_prefix(RESOURCE_ARTIFACT_URI_PREFIX)
        .and_then(|id| id.parse::<i64>().ok())
    {
        return Some(format!(
            "{}/{}",
            API_ARTIFACTS.trim_start_matches('/'),
            artifact_id
        ));
    }
    None
}

pub(crate) fn resource_entries_from_workflow_runs(workflow_runs: &[Value]) -> Vec<Value> {
    let mut resources = Vec::new();
    for run in workflow_runs {
        let Some(run_id) = run.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(STATUS_UNKNOWN);
        resources.push(json!({
            "uri": format!("{RESOURCE_WORKFLOW_RUN_URI_PREFIX}{run_id}"),
            "name": format!("Workflow run {run_id}: {status}"),
            "mimeType": "application/json",
        }));
    }
    resources
}

pub(crate) fn resource_entries_from_runs(runs: &[Value]) -> Vec<Value> {
    let mut resources = Vec::new();
    for run in runs {
        let Some(run_id) = run.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(STATUS_UNKNOWN);
        resources.push(json!({
            "uri": format!("{RESOURCE_RUN_URI_PREFIX}{run_id}"),
            "name": format!("Run {run_id}: {status}"),
            "mimeType": "application/json",
        }));
        resources.push(json!({
            "uri": format!("{RESOURCE_RUN_URI_PREFIX}{run_id}/chunks"),
            "name": format!("Run {run_id} chunks"),
            "mimeType": "application/json",
        }));
        resources.push(json!({
            "uri": format!("{RESOURCE_RUN_URI_PREFIX}{run_id}/artifacts"),
            "name": format!("Run {run_id} artifacts"),
            "mimeType": "application/json",
        }));
    }
    resources
}
