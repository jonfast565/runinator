//! what a run left behind, addressable by URI.
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
const EFFECT_PREFIX: &str = "runinator://effects/";

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
            "description": "One workflow run with its VM continuations, effects, and journal.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{RUN_PREFIX}{{id}}/artifacts"),
            "name": "Workflow run artifacts",
            "description": "The artifacts a workflow run produced.",
            "mimeType": "application/json",
        }),
        json!({
            "uri": format!("{EFFECT_PREFIX}{{id}}/output"),
            "name": "Effect output",
            "description": "The chunks and artifacts one workflow effect produced.",
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

/// Read one resource and return the JSON named by its URI.
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
        let effects = client
            .fetch_workflow_effects(run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        let mut artifacts = Vec::new();
        for effect in effects {
            artifacts.extend(
                client
                    .fetch_workflow_effect_output(effect.id)
                    .await
                    .map_err(|failure| failure.to_string())?
                    .into_iter()
                    .filter(|event| {
                        matches!(
                            event.output,
                            runinator_models::workflow_vm::WorkflowEffectOutput::Artifact { .. }
                        )
                    }),
            );
        }
        return to_value(&artifacts);
    }
    if let Some(run_id) = uuid_after(uri, RUN_PREFIX, "") {
        let run = client
            .fetch_workflow_runs(None, None)
            .await
            .map_err(|failure| failure.to_string())?
            .into_iter()
            .find(|run| run.id == run_id)
            .ok_or_else(|| format!("workflow run {run_id} not found"))?;
        let continuations = client
            .fetch_workflow_continuations(run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        let effects = client
            .fetch_workflow_effects(run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        let journal = client
            .fetch_workflow_journal(run_id)
            .await
            .map_err(|failure| failure.to_string())?;
        return Ok(json!({
            "run": run,
            "continuations": continuations,
            "effects": effects,
            "journal": journal,
        }));
    }
    if let Some(effect_id) = uuid_after(uri, EFFECT_PREFIX, "/output") {
        let output = client
            .fetch_workflow_effect_output(effect_id)
            .await
            .map_err(|failure| failure.to_string())?;
        return to_value(&output);
    }
    Err(format!("unsupported resource uri '{uri}'"))
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value)
        .map(Value::from)
        .map_err(|failure| failure.to_string())
}

/// Return the UUID between `prefix` and `suffix` when the URI has exactly that shape.
fn uuid_after(uri: &str, prefix: &str, suffix: &str) -> Option<Uuid> {
    uri.strip_prefix(prefix)?
        .strip_suffix(suffix)
        .and_then(|raw| raw.parse().ok())
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
