use std::{
    io::{self, BufRead, Write},
    thread,
    time::Duration,
};

mod contracts;
mod protocol;
mod resources;
mod tools;

use clap::Parser;
use reqwest::blocking::Client;
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    api_routes::{
        API_PACKS_IMPORT, API_PROVIDERS, API_RUNS, API_WORKFLOW_RUNS, API_WORKFLOWS,
        API_WORKFLOWS_EXPORT, API_WORKFLOWS_IMPORT, API_WORKFLOWS_VALIDATE,
        WORKFLOW_JSON_IMPORT_RISK_ACK, WORKFLOW_JSON_IMPORT_RISK_HEADER, api_run_artifacts,
        api_workflow, api_workflow_run, api_workflow_runs,
    },
    runs::RunStatus,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowStatus},
};

use contracts::{RESOURCE_ARTIFACT_URI_PREFIX, RESOURCE_WORKFLOW_RUN_URI_PREFIX};
use protocol::{json_export_response, json_tool_response, required_uuid, required_value};
use resources::{
    resource_entries_from_runs, resource_entries_from_workflow_runs, resource_path_for_uri,
    resource_templates,
};
use tools::{fixed_tools, parse_tool_workflow_id, tools_from_workflows};
use uuid::Uuid;

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
            .post(self.url(&api_workflow_runs(workflow_id)))
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
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<Uuid>().ok());
        let workflow_run_id_display = workflow_run_id.map(|id| id.to_string()).unwrap_or_default();
        if let Some(workflow_run_id) = workflow_run_id
            && let Some(completed) = self.wait_for_quick_completion(workflow_run_id)?
        {
            workflow_run = completed;
            response = json!({ "run": workflow_run, "nodes": [] });
        }

        let status = workflow_run
            .get("status")
            .and_then(Value::as_str)
            .and_then(|status| WorkflowStatus::try_from(status).ok())
            .unwrap_or(WorkflowStatus::Queued);
        if status.is_terminal() {
            let is_error = status != WorkflowStatus::Succeeded;
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Runinator workflow finished with status {}. Workflow run resource: {RESOURCE_WORKFLOW_RUN_URI_PREFIX}{workflow_run_id_display}", status.as_str())
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
                "text": format!("Runinator workflow queued. Workflow run resource: {RESOURCE_WORKFLOW_RUN_URI_PREFIX}{workflow_run_id_display}")
            }],
            "structuredContent": response,
            "isError": false,
        }))
    }

    fn fixed_tool_call(&self, name: &str, arguments: Value) -> Result<Option<Value>, String> {
        let result = match name {
            "runinator_list_providers" => {
                let providers = self.fetch_api_json(API_PROVIDERS)?;
                json_tool_response("Provider metadata", providers, false)?
            }
            "runinator_list_workflows" => {
                let workflows = self.fetch_api_json(API_WORKFLOWS)?;
                json_tool_response("Workflow definitions", workflows, false)?
            }
            "runinator_get_workflow" => {
                let workflow_id = required_uuid(&arguments, "workflow_id")?;
                let workflow = self.fetch_api_json(&api_workflow(workflow_id))?;
                json_tool_response("Workflow definition", workflow, false)?
            }
            "runinator_validate_workflow" => {
                let workflow = required_value(&arguments, "workflow")?;
                let workflow = self.post_api_json(API_WORKFLOWS_VALIDATE, workflow)?;
                json_tool_response("Workflow validates", workflow, false)?
            }
            "runinator_save_workflow" => {
                let workflow = required_value(&arguments, "workflow")?;
                let saved = if let Some(workflow_id) = workflow
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|raw| raw.parse::<Uuid>().ok())
                {
                    self.patch_api_json(&api_workflow(workflow_id), workflow)?
                } else {
                    self.post_api_json(API_WORKFLOWS, workflow)?
                };
                json_tool_response("Workflow saved", saved, false)?
            }
            "runinator_import_workflow_bundle" => {
                if arguments_are_json_workflow_bundle(&arguments) {
                    if arguments
                        .get("acknowledge_system_breakage")
                        .and_then(Value::as_bool)
                        != Some(true)
                    {
                        return Err("json workflow bundle import requires acknowledge_system_breakage=true because system breakage is possible".to_string());
                    }
                    let bundle: WorkflowBundle = serde_json::from_value(arguments.clone().into())
                        .map_err(|err| err.to_string())?;
                    let result = self.post_workflow_bundle_json(API_WORKFLOWS_IMPORT, &bundle)?;
                    return Ok(Some(json_tool_response(
                        "Workflow JSON bundle imported",
                        result,
                        false,
                    )?));
                }
                // a full pack: one or more WDL sources plus optional `.wdls` secrets, compiled
                // client-side into a single pack zip.
                let sources = collect_wdl_sources(&arguments)?;
                let options = runinator_wdl::CompileOptions {
                    enabled: true,
                    default_version: runinator_models::semver::SemVer::default(),
                };
                let mut workflows = Vec::with_capacity(sources.len());
                for source in &sources {
                    let mut definition = runinator_wdl::compile_str(source, &options)
                        .map_err(|err| err.to_string())?;
                    // an explicit ad-hoc import should always win the import reconciliation.
                    definition.updated_at = Some(chrono::Utc::now());
                    workflows.push(definition);
                }
                let secrets = match arguments.get("secrets").and_then(Value::as_str) {
                    Some(text) if !text.trim().is_empty() => Some(
                        runinator_wdl::parse_secrets_str(text).map_err(|err| err.to_string())?,
                    ),
                    _ => None,
                };
                let bundle = WorkflowBundle {
                    workflows,
                    triggers: Vec::new(),
                };
                let body = runinator_utilities::pack::build_pack_zip(&bundle, secrets.as_ref())
                    .map_err(|err| err.to_string())?;
                let result = self.post_api_zip(API_PACKS_IMPORT, body)?;
                json_tool_response("Workflow pack imported", result, false)?
            }
            "runinator_export_workflow_bundle" => {
                let path = match arguments
                    .get("workflow_id")
                    .and_then(Value::as_str)
                    .and_then(|raw| raw.parse::<Uuid>().ok())
                {
                    Some(workflow_id) => format!("{}/export", api_workflow(workflow_id)),
                    None => API_WORKFLOWS_EXPORT.to_string(),
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

    fn post_api_zip(&self, path: &str, body: Vec<u8>) -> Result<Value, String> {
        self.client
            .post(self.url(path))
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .body(body)
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())
    }

    fn post_workflow_bundle_json(
        &self,
        path: &str,
        body: &WorkflowBundle,
    ) -> Result<Value, String> {
        self.client
            .post(self.url(path))
            .header(
                WORKFLOW_JSON_IMPORT_RISK_HEADER,
                WORKFLOW_JSON_IMPORT_RISK_ACK,
            )
            .json(body)
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

    fn wait_for_quick_completion(&self, run_id: Uuid) -> Result<Option<Value>, String> {
        for _ in 0..10 {
            let run = self.fetch_api_json(&api_workflow_run(run_id))?;
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
            .get(self.url(API_WORKFLOWS))
            .send()
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .map_err(|err| err.to_string())
    }

    fn recent_resource_entries(&self) -> Result<Vec<Value>, String> {
        let mut workflow_runs = match self.fetch_api_json(API_WORKFLOW_RUNS) {
            Ok(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        workflow_runs.sort_by_key(|run| {
            run.get("created_at")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        });
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
                self.fetch_api_json(&format!("{API_RUNS}?status={}", status.as_str()))
            {
                runs.extend(items);
            }
        }
        runs.sort_by_key(|run| {
            run.get("created_at")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        });
        runs.reverse();
        runs.truncate(20);

        let mut resources = resource_entries_from_workflow_runs(&workflow_runs);
        resources.extend(resource_entries_from_runs(&runs));
        for run in &runs {
            let Some(run_id) = run
                .get("id")
                .and_then(Value::as_str)
                .and_then(|raw| raw.parse::<Uuid>().ok())
            else {
                continue;
            };
            if let Ok(Value::Array(artifacts)) = self.fetch_api_json(&api_run_artifacts(run_id)) {
                for artifact in artifacts {
                    let Some(artifact_id) = artifact.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let name = artifact
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Artifact");
                    resources.push(json!({
                        "uri": format!("{RESOURCE_ARTIFACT_URI_PREFIX}{artifact_id}"),
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

/// collect WDL sources from import arguments: a `workflows` array of WDL strings, or a single
/// `source` string for convenience.
fn collect_wdl_sources(arguments: &Value) -> Result<Vec<String>, String> {
    if let Some(array) = arguments.get("workflows").and_then(Value::as_array) {
        let sources: Vec<String> = array
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        if sources.is_empty() {
            return Err("import requires at least one WDL source in 'workflows'".to_string());
        }
        return Ok(sources);
    }
    if let Some(source) = arguments.get("source").and_then(Value::as_str) {
        return Ok(vec![source.to_string()]);
    }
    Err(
        "import requires 'workflows' (array of WDL sources) or a single 'source' string"
            .to_string(),
    )
}

fn arguments_are_json_workflow_bundle(arguments: &Value) -> bool {
    if arguments.get("triggers").is_some() {
        return true;
    }
    arguments
        .get("workflows")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(Value::is_object))
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
