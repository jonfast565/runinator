use std::{
    io::{self, BufRead, Write},
    thread,
    time::Duration,
};

use clap::Parser;
use reqwest::blocking::Client;
use runinator_models::{
    core::ScheduledTask,
    runs::{RunRequest, RunStatus},
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
        let tasks = self.fetch_tasks()?;
        Ok(json!({ "tools": tools_from_tasks(tasks) }))
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

        let task_id = parse_tool_task_id(name)
            .ok_or_else(|| format!("Tool name '{name}' does not contain a task id"))?;
        let request = RunRequest {
            parameters: arguments,
            trigger: "mcp".into(),
        };

        let mut run: Value = self
            .client
            .post(self.url(&format!("tasks/{task_id}/runs")))
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
            let chunks = self.fetch_api_json(&format!("runs/{run_id}/chunks?limit=500"))?;
            let artifacts = self.fetch_api_json(&format!("runs/{run_id}/artifacts"))?;
            let is_error = status != "succeeded";
            let output = run.get("output_json").cloned().unwrap_or(Value::Null);
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Runinator task finished with status {status}. Run resource: runinator://runs/{run_id}")
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
                "text": format!("Runinator task queued. Run resource: runinator://runs/{run_id}")
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

    fn fetch_tasks(&self) -> Result<Vec<ScheduledTask>, String> {
        self.client
            .get(self.url("tasks"))
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
            "{}{}",
            self.api_base_url.trim_end_matches('/'),
            format!("/{path}")
        )
    }
}

fn tools_from_tasks(tasks: Vec<ScheduledTask>) -> Vec<Value> {
    tasks
        .into_iter()
        .filter(|task| task.enabled && task.mcp_enabled)
        .filter_map(|task| {
            let id = task.id?;
            Some(json!({
                "name": tool_name(&task, id),
                "description": format!("Run Runinator task '{}'", task.name),
                "inputSchema": task.input_schema,
            }))
        })
        .collect()
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
            "uri": "runinator://artifacts/{id}",
            "name": "Artifact metadata",
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

fn tool_name(task: &ScheduledTask, id: i64) -> String {
    let slug = task
        .name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    format!("task_{id}_{slug}")
}

fn parse_tool_task_id(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("task_")?;
    let (raw_id, _) = rest.split_once('_')?;
    raw_id.parse().ok()
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

    fn task(id: i64, name: &str, enabled: bool, mcp_enabled: bool) -> ScheduledTask {
        ScheduledTask {
            id: Some(id),
            name: name.into(),
            cron_schedule: "* * * * *".into(),
            action_name: "Console".into(),
            action_function: "exec".into(),
            action_configuration: "true".into(),
            timeout: 30,
            next_execution: None,
            enabled,
            immediate: false,
            blackout_start: None,
            blackout_end: None,
            input_schema: json!({ "type": "object" }),
            default_parameters: json!({}),
            output_schema: None,
            mcp_enabled,
            metadata: json!({}),
            tags: Vec::new(),
        }
    }

    #[test]
    fn tools_include_only_enabled_mcp_tasks() {
        let tools = tools_from_tasks(vec![
            task(1, "Allowed", true, true),
            task(2, "Disabled", false, true),
            task(3, "Hidden", true, false),
        ]);

        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("name").and_then(Value::as_str),
            Some("task_1_allowed")
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
