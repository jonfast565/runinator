use std::{
    io::{self, BufRead, Write},
    thread,
    time::Duration,
};

use clap::Parser;
use reqwest::blocking::Client;
use runinator_models::{runs::RunStatus, workflows::WorkflowDefinition};
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
        Ok(json!({ "tools": tools_from_workflows(workflows) }))
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

        let workflow_id = parse_tool_workflow_id(name)
            .ok_or_else(|| format!("Tool name '{name}' does not contain a workflow id"))?;
        let request = json!({
            "parameters": arguments
        });

        let mut run: Value = self
            .client
            .post(self.url(&format!("workflows/{workflow_id}/runs")))
            .json(&request)
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())?;

        let run_id = run.get("id").and_then(Value::as_i64).unwrap_or_default();
        if run_id > 0 {
            if let Some(completed) = self.wait_for_quick_completion(run_id)? {
                run = completed;
            }
        }

        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("queued");
        if matches!(status, "succeeded" | "failed" | "timed_out" | "canceled") {
            let chunks = self
                .fetch_api_json(&format!("runs/{run_id}/chunks?limit=500"))
                .unwrap_or(json!([]));
            let artifacts = self
                .fetch_api_json(&format!("runs/{run_id}/artifacts"))
                .unwrap_or(json!([]));
            let is_error = status != "succeeded";
            let output = run.get("output_json").cloned().unwrap_or(Value::Null);
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Runinator workflow finished with status {status}. Run resource: runinator://runs/{run_id}")
                }],
                "structuredContent": {
                    "run": run,
                    "output": output,
                    "chunks": chunks,
                    "artifacts": artifacts,
                },
                "isError": is_error,
            }));
        }

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Runinator workflow queued. Run resource: runinator://runs/{run_id}")
            }],
            "structuredContent": run,
            "isError": false,
        }))
    }

    fn resources_read(&self, params: Value) -> Result<Value, String> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| "resources/read missing params.uri".to_string())?;
        if let Some(raw) = uri.strip_prefix("runinator://runs/") {
            if let Some(run_id) = raw
                .strip_suffix("/chunks")
                .and_then(|id| id.parse::<i64>().ok())
            {
                return self.read_api_resource(uri, &format!("runs/{run_id}/chunks?limit=500"));
            }
            if let Some(run_id) = raw
                .strip_suffix("/artifacts")
                .and_then(|id| id.parse::<i64>().ok())
            {
                return self.read_api_resource(uri, &format!("runs/{run_id}/artifacts"));
            }
            if let Ok(run_id) = raw.parse::<i64>() {
                return self.read_api_resource(uri, &format!("runs/{run_id}"));
            }
        }
        if let Some(artifact_id) = uri
            .strip_prefix("runinator://artifacts/")
            .and_then(|id| id.parse::<i64>().ok())
        {
            return self.read_api_resource(uri, &format!("artifacts/{artifact_id}"));
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

    fn wait_for_quick_completion(&self, run_id: i64) -> Result<Option<Value>, String> {
        for _ in 0..10 {
            let run = self.fetch_api_json(&format!("runs/{run_id}"))?;
            let status = run
                .get("status")
                .and_then(Value::as_str)
                .and_then(|status| RunStatus::try_from(status).ok());
            if status.is_some_and(|status| {
                matches!(
                    status,
                    RunStatus::Succeeded
                        | RunStatus::Failed
                        | RunStatus::TimedOut
                        | RunStatus::Canceled
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

        let mut resources = resource_entries_from_runs(&runs);
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
        json!({
            "uri": "runinator://workflows",
            "name": "Workflow list",
            "mimeType": "application/json"
        }),
    ]
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
mod tests {
    use super::*;

    fn mock_workflow(id: i64, name: &str, enabled: bool) -> WorkflowDefinition {
        WorkflowDefinition {
            id: Some(id),
            name: name.into(),
            version: 1,
            enabled,
            input_schema: json!({ "type": "object" }),
            definition: json!({}),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn tools_include_only_enabled_workflows() {
        let tools = tools_from_workflows(vec![
            mock_workflow(1, "Allowed", true),
            mock_workflow(2, "Disabled", false),
        ]);

        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("name").and_then(Value::as_str),
            Some("allowed_1")
        );
    }

    #[test]
    fn admin_mutation_methods_are_rejected() {
        let server = McpServer::new("http://127.0.0.1:9/".into()).unwrap();
        let response = server.handle(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tasks/create",
            "params": {}
        }));

        assert!(response.get("error").is_some());
    }

    #[test]
    fn resource_entries_include_run_children() {
        let entries = resource_entries_from_runs(&[json!({ "id": 7, "status": "succeeded" })]);
        let uris = entries
            .iter()
            .filter_map(|entry| entry.get("uri").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert!(uris.contains(&"runinator://runs/7"));
        assert!(uris.contains(&"runinator://runs/7/chunks"));
        assert!(uris.contains(&"runinator://runs/7/artifacts"));
    }
}
