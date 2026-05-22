use std::{
    io::{self, BufRead, Write},
    thread,
    time::Duration,
};

use clap::Parser;
use reqwest::blocking::Client;
use runinator_models::{
    runs::RunStatus,
    workflows::{WorkflowDefinition, WorkflowStatus},
};
use serde_json::{Value, json};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080/")]
    api_base_url: String,
}

struct McpServer {
    api_base_url: String,
    client: Client,
}

impl McpServer {
    fn new(api_base_url: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            api_base_url,
            client: Client::builder().build()?,
        })
    }

    fn handle(&self, request: Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2025-03-26",
                "serverInfo": { "name": "runinator-mcp", "version": "0.1.0" },
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false }
                }
            })),
            "tools/list" => self.tools_list(),
            "tools/call" => self.tools_call(request.get("params").cloned().unwrap_or(Value::Null)),
            "resources/list" => self.resources_list(),
            "resources/read" => {
                self.resources_read(request.get("params").cloned().unwrap_or(Value::Null))
            }
            _ => Err(format!("Unsupported MCP method '{method}'")),
        };

        match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(message) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32603, "message": message }
            }),
        }
    }

    fn tools_list(&self) -> Result<Value, String> {
        let workflows = self.fetch_workflows()?;
        let mut tools = fixed_tools();
        tools.extend(tools_from_workflows(workflows));
        Ok(json!({ "tools": tools }))
    }

    fn tools_call(&self, params: Value) -> Result<Value, String> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "tools/call missing params.name".to_string())?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));

        if let Some(result) = self.fixed_tool_call(name, arguments.clone())? {
            return Ok(result);
        }

        let workflow_id = parse_tool_workflow_id(name)
            .ok_or_else(|| format!("Tool name '{name}' does not contain a workflow id"))?;
        let request = json!({
            "parameters": arguments
        });

        let mut response: Value = self
            .client
            .post(self.url(&format!("workflows/{workflow_id}/runs")))
            .json(&request)
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())?;

        let mut workflow_run = response
            .get("run")
            .cloned()
            .unwrap_or_else(|| response.clone());
        let workflow_run_id = workflow_run
            .get("id")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if workflow_run_id > 0 {
            if let Some(completed) = self.wait_for_quick_completion(workflow_run_id)? {
                workflow_run = completed;
                response = json!({ "run": workflow_run, "nodes": [] });
            }
        }

        let status = workflow_run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("queued");
        if matches!(status, "succeeded" | "failed" | "timed_out" | "canceled") {
            let is_error = status != "succeeded";
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Runinator workflow finished with status {status}. Workflow run resource: runinator://workflow_runs/{workflow_run_id}")
                }],
                "structuredContent": {
                    "workflow_run": workflow_run,
                },
                "isError": is_error,
            }));
        }

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Runinator workflow queued. Workflow run resource: runinator://workflow_runs/{workflow_run_id}")
            }],
            "structuredContent": response,
            "isError": false,
        }))
    }

    fn fixed_tool_call(&self, name: &str, arguments: Value) -> Result<Option<Value>, String> {
        let result = match name {
            "runinator_list_providers" => {
                let providers = self.fetch_api_json("providers")?;
                json_tool_response("Provider metadata", providers, false)?
            }
            "runinator_list_workflows" => {
                let workflows = self.fetch_api_json("workflows")?;
                json_tool_response("Workflow definitions", workflows, false)?
            }
            "runinator_get_workflow" => {
                let workflow_id = required_i64(&arguments, "workflow_id")?;
                let workflow = self.fetch_api_json(&format!("workflows/{workflow_id}"))?;
                json_tool_response("Workflow definition", workflow, false)?
            }
            "runinator_validate_workflow" => {
                let workflow = required_value(&arguments, "workflow")?;
                let workflow = self.post_api_json("workflows/validate", workflow)?;
                json_tool_response("Workflow validates", workflow, false)?
            }
            "runinator_save_workflow" => {
                let workflow = required_value(&arguments, "workflow")?;
                let saved = if let Some(workflow_id) = workflow.get("id").and_then(Value::as_i64) {
                    self.patch_api_json(&format!("workflows/{workflow_id}"), workflow)?
                } else {
                    self.post_api_json("workflows", workflow)?
                };
                json_tool_response("Workflow saved", saved, false)?
            }
            "runinator_import_workflow_bundle" => {
                let bundle = self.post_api_json("workflows/import", arguments)?;
                json_tool_response("Workflow bundle imported", bundle, false)?
            }
            "runinator_export_workflow_bundle" => {
                let path = match arguments.get("workflow_id").and_then(Value::as_i64) {
                    Some(workflow_id) => format!("workflows/{workflow_id}/export"),
                    None => "workflows/export".to_string(),
                };
                let bundle = self.fetch_api_json(&path)?;
                json_export_response(bundle)?
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    fn resources_read(&self, params: Value) -> Result<Value, String> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| "resources/read missing params.uri".to_string())?;
        if let Some(path) = resource_path_for_uri(uri) {
            return self.read_api_resource(uri, &path);
        }
        Err(format!("Unsupported resource URI '{uri}'"))
    }

    fn resources_list(&self) -> Result<Value, String> {
        let mut resources = resource_templates();
        resources.extend(self.recent_resource_entries().unwrap_or_default());
        Ok(json!({ "resources": resources }))
    }

    fn read_api_resource(&self, uri: &str, path: &str) -> Result<Value, String> {
        let body: Value = self
            .client
            .get(self.url(path))
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())?;
        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&body).map_err(|err| err.to_string())?
            }]
        }))
    }

    fn fetch_api_json(&self, path: &str) -> Result<Value, String> {
        self.client
            .get(self.url(path))
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())
    }

    fn post_api_json(&self, path: &str, body: Value) -> Result<Value, String> {
        self.client
            .post(self.url(path))
            .json(&body)
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())
    }

    fn patch_api_json(&self, path: &str, body: Value) -> Result<Value, String> {
        self.client
            .patch(self.url(path))
            .json(&body)
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())
    }

    fn wait_for_quick_completion(&self, run_id: i64) -> Result<Option<Value>, String> {
        for _ in 0..10 {
            let run = self.fetch_api_json(&format!("workflow_runs/{run_id}"))?;
            let status = run
                .get("status")
                .and_then(Value::as_str)
                .and_then(|status| WorkflowStatus::try_from(status).ok());
            if status.is_some_and(|status| {
                matches!(
                    status,
                    WorkflowStatus::Succeeded
                        | WorkflowStatus::Failed
                        | WorkflowStatus::TimedOut
                        | WorkflowStatus::Canceled
                )
            }) {
                return Ok(Some(run));
            }
            thread::sleep(Duration::from_millis(250));
        }
        Ok(None)
    }

    fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>, String> {
        self.client
            .get(self.url("workflows"))
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())
    }

    fn recent_resource_entries(&self) -> Result<Vec<Value>, String> {
        let mut workflow_runs = match self.fetch_api_json("workflow_runs") {
            Ok(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        workflow_runs.sort_by_key(|run| run.get("id").and_then(Value::as_i64).unwrap_or_default());
        workflow_runs.reverse();
        workflow_runs.truncate(20);

        let mut runs = Vec::new();
        for status in [
            RunStatus::Running,
            RunStatus::Queued,
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::TimedOut,
            RunStatus::Canceled,
        ] {
            if let Ok(Value::Array(items)) =
                self.fetch_api_json(&format!("runs?status={}", status.as_str()))
            {
                runs.extend(items);
            }
        }
        runs.sort_by_key(|run| run.get("id").and_then(Value::as_i64).unwrap_or_default());
        runs.reverse();
        runs.truncate(20);

        let mut resources = resource_entries_from_workflow_runs(&workflow_runs);
        resources.extend(resource_entries_from_runs(&runs));
        for run in &runs {
            let Some(run_id) = run.get("id").and_then(Value::as_i64) else {
                continue;
            };
            if let Ok(Value::Array(artifacts)) =
                self.fetch_api_json(&format!("runs/{run_id}/artifacts"))
            {
                for artifact in artifacts {
                    let Some(artifact_id) = artifact.get("id").and_then(Value::as_i64) else {
                        continue;
                    };
                    let name = artifact
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Artifact");
                    resources.push(json!({
                        "uri": format!("runinator://artifacts/{artifact_id}"),
                        "name": format!("Artifact {artifact_id}: {name}"),
                        "mimeType": "application/json",
                    }));
                }
            }
        }
        Ok(resources)
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.api_base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

fn tools_from_workflows(workflows: Vec<WorkflowDefinition>) -> Vec<Value> {
    workflows
        .into_iter()
        .filter(|wf| wf.enabled)
        .filter_map(|wf| {
            let id = wf.id?;
            Some(json!({
                "name": tool_name(&wf, id),
                "description": format!("Execute workflow: {}", wf.name),
                "inputSchema": wf.input_schema,
            }))
        })
        .collect()
}

fn fixed_tools() -> Vec<Value> {
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
    let mut property_map = serde_json::Map::new();
    for (name, schema) in properties {
        property_map.insert(name.into(), schema);
    }
    json!({
        "type": "object",
        "properties": property_map,
        "required": required,
    })
}

fn required_i64(arguments: &Value, name: &str) -> Result<i64, String> {
    arguments
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer argument '{name}'"))
}

fn required_value(arguments: &Value, name: &str) -> Result<Value, String> {
    arguments
        .get(name)
        .cloned()
        .ok_or_else(|| format!("missing argument '{name}'"))
}

fn json_tool_response(message: &str, value: Value, is_error: bool) -> Result<Value, String> {
    Ok(json!({
        "content": [{
            "type": "text",
            "text": message,
        }],
        "structuredContent": value,
        "isError": is_error,
    }))
}

fn json_export_response(bundle: Value) -> Result<Value, String> {
    let text = serde_json::to_string_pretty(&bundle).map_err(|err| err.to_string())?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": text,
        }],
        "structuredContent": bundle,
        "isError": false,
    }))
}

fn parse_tool_workflow_id(name: &str) -> Option<i64> {
    name.split('_').last()?.parse().ok()
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

fn resource_templates() -> Vec<Value> {
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

fn resource_path_for_uri(uri: &str) -> Option<String> {
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

fn resource_entries_from_workflow_runs(workflow_runs: &[Value]) -> Vec<Value> {
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

fn resource_entries_from_runs(runs: &[Value]) -> Vec<Value> {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let server = McpServer::new(args.api_base_url)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                writeln!(
                    stdout,
                    "{}",
                    json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32700, "message": err.to_string() }
                    })
                )?;
                stdout.flush()?;
                continue;
            }
        };
        writeln!(stdout, "{}", server.handle(request))?;
        stdout.flush()?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
