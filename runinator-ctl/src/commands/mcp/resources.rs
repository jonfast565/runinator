//! what a run left behind, addressable by uri.
//!
//! a tool call is the model asking runinator to do something; a resource is the model reading what
//! already happened. everything here is reachable through `runinator_exec` too — the point of the
//! second surface is that a client can attach a run's logs to the conversation without spending a
//! tool call to fetch them.

use runinator_models::json;
use runinator_models::value::Value;
use uuid::Uuid;

use crate::commands::Client;

const WORKFLOWS_URI: &str = "runinator://workflows";
const WORKFLOW_PREFIX: &str = "runinator://workflows/";
const RUN_PREFIX: &str = "runinator://runs/";
const NODE_RUN_PREFIX: &str = "runinator://node_runs/";

/// how many log chunks a node-run resource carries.
const CHUNK_LIMIT: i64 = 500;

/// the addressable shapes, listed for a client that has no run in mind yet.
pub(crate) fn templates() -> Vec<Value> {
    vec![
        json!({
            "uri": WORKFLOWS_URI,
            "name": "Workflow list",
            "description": "Every saved workflow definition.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{WORKFLOW_PREFIX}{{id}}"),
            "name": "Workflow definition",
            "description": "One workflow definition by id.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{RUN_PREFIX}{{id}}"),
            "name": "Workflow run",
            "description": "One workflow run with its node runs.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{RUN_PREFIX}{{id}}/artifacts"),
            "name": "Workflow run artifacts",
            "description": "The artifacts a workflow run produced.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{NODE_RUN_PREFIX}{{id}}/chunks"),
            "name": "Node run logs",
            "description": "The log chunks one node run wrote.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{NODE_RUN_PREFIX}{{id}}/artifacts"),
            "name": "Node run artifacts",
            "description": "The artifacts one node run produced.",
            "mimeType": "application/json",
        }),
    ]
}

/// the templates, plus the runs that are actually there right now.
pub(crate) async fn list(client: &Client) -> Vec<Value> {
    let mut resources = templates();
    let Ok(runs) = client.fetch_workflow_runs(None, None).await else {
        return resources;
    };
    let mut runs = runs;
    runs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    runs.truncate(25);
    for run in runs {
        resources.push(json!({
            "uri": format!("{RUN_PREFIX}{}", run.id),
            "name": format!("Workflow run {} [{}]", run.id, run.status.as_str()),
            "mimeType": "application/json",
        }));
    }
    resources
}

/// read one resource, as the json its uri names.
pub(crate) async fn read(client: &Client, uri: &str) -> Result<Value, String> {
    let body = fetch(client, uri).await?;
    let text = serde_json::to_string_pretty(&body).map_err(|failure| failure.to_string())?;
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": text,
        }]
    }))
}

async fn fetch(client: &Client, uri: &str) -> Result<Value, String> {
    if uri == WORKFLOWS_URI {
        let workflows = client
            .fetch_workflows()
            .await
            .map_err(|failure| failure.to_string())?;
        return to_value(&workflows);
    }
    if let Some(workflow_id) = uuid_after(uri, WORKFLOW_PREFIX, "") {
        let workflow = client
            .fetch_workflow(workflow_id)
            .await
            .map_err(|failure| failure.to_string())?;
        return to_value(&workflow);
    }
    if let Some(run_id) = uuid_after(uri, RUN_PREFIX, "/artifacts") {
        let artifacts = client
            .fetch_workflow_run_artifacts(run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        return to_value(&artifacts);
    }
    if let Some(run_id) = uuid_after(uri, RUN_PREFIX, "") {
        let (run, nodes) = client
            .fetch_workflow_run(run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        return Ok(json!({ "run": run, "nodes": nodes }));
    }
    if let Some(node_run_id) = uuid_after(uri, NODE_RUN_PREFIX, "/chunks") {
        let chunks = client
            .fetch_workflow_node_run_chunks(node_run_id, None, CHUNK_LIMIT)
            .await
            .map_err(|failure| failure.to_string())?;
        return to_value(&chunks);
    }
    if let Some(node_run_id) = uuid_after(uri, NODE_RUN_PREFIX, "/artifacts") {
        let artifacts = client
            .fetch_workflow_node_run_artifacts(node_run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        return to_value(&artifacts);
    }
    Err(format!("unsupported resource uri '{uri}'"))
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value)
        .map(Value::from)
        .map_err(|failure| failure.to_string())
}

/// the uuid between `prefix` and `suffix`, when the uri is exactly that shape.
fn uuid_after(uri: &str, prefix: &str, suffix: &str) -> Option<Uuid> {
    uri.strip_prefix(prefix)?
        .strip_suffix(suffix)
        .and_then(|raw| raw.parse().ok())
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
