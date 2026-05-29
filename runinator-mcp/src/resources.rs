use runinator_models::json;
use runinator_models::value::Value;

pub(crate) fn resource_templates() -> Vec<Value> {
    vec![
        json!({
            "uri": "runinator://workflows",
            "name": "Workflow list",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "runinator://workflows/{id}",
            "name": "Workflow definition",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "runinator://workflow_runs/{id}",
            "name": "Workflow run",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "runinator://runs/{id}",
            "name": "Run summary",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "runinator://runs/{id}/chunks",
            "name": "Run output chunks",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "runinator://runs/{id}/artifacts",
            "name": "Run artifacts",
            "mimeType": "application/json"
        }),
    ]
}

pub(crate) fn resource_path_for_uri(uri: &str) -> Option<String> {
    if uri == "runinator://workflows" {
        return Some("workflows".into());
    }
    if let Some(workflow_id) = uri
        .strip_prefix("runinator://workflows/")
        .and_then(|id| id.parse::<i64>().ok())
    {
        return Some(format!("workflows/{workflow_id}"));
    }
    if let Some(workflow_run_id) = uri
        .strip_prefix("runinator://workflow_runs/")
        .and_then(|id| id.parse::<i64>().ok())
    {
        return Some(format!("workflow_runs/{workflow_run_id}"));
    }
    if let Some(raw) = uri.strip_prefix("runinator://runs/") {
        if let Some(run_id) = raw
            .strip_suffix("/chunks")
            .and_then(|id| id.parse::<i64>().ok())
        {
            return Some(format!("runs/{run_id}/chunks?limit=500"));
        }
        if let Some(run_id) = raw
            .strip_suffix("/artifacts")
            .and_then(|id| id.parse::<i64>().ok())
        {
            return Some(format!("runs/{run_id}/artifacts"));
        }
        if let Ok(run_id) = raw.parse::<i64>() {
            return Some(format!("runs/{run_id}"));
        }
    }
    if let Some(artifact_id) = uri
        .strip_prefix("runinator://artifacts/")
        .and_then(|id| id.parse::<i64>().ok())
    {
        return Some(format!("artifacts/{artifact_id}"));
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
            .unwrap_or("unknown");
        resources.push(json!({
            "uri": format!("runinator://workflow_runs/{run_id}"),
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
            .unwrap_or("unknown");
        resources.push(json!({
            "uri": format!("runinator://runs/{run_id}"),
            "name": format!("Run {run_id}: {status}"),
            "mimeType": "application/json",
        }));
        resources.push(json!({
            "uri": format!("runinator://runs/{run_id}/chunks"),
            "name": format!("Run {run_id} chunks"),
            "mimeType": "application/json",
        }));
        resources.push(json!({
            "uri": format!("runinator://runs/{run_id}/artifacts"),
            "name": format!("Run {run_id} artifacts"),
            "mimeType": "application/json",
        }));
    }
    resources
}
