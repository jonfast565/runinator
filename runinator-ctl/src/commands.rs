use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::{
    bundles::SecretBundle,
    providers::ProviderMetadata,
    settings::SettingKind,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus,
        WorkflowTrigger,
    },
};
use tokio::time;

use crate::{
    cli::{
        ApprovalCommands, Cli, Commands, ProviderCommands, RunCommands, SettingsCommands,
        TriggerCommands, WdlCommands, WorkflowCommands,
    },
    output, params,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

type Client = AsyncApiClient<StaticLocator>;

pub fn err(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

pub async fn run(client: &Client, cli: &Cli) -> Result<()> {
    match &cli.command {
        Commands::Status => status(client, cli.json).await,
        Commands::Workflows { command } => workflows(client, command, cli.json).await,
        Commands::Runs { command } => runs(client, command, cli.json).await,
        Commands::Approvals { command } => approvals(client, command, cli.json).await,
        Commands::Triggers { command } => triggers(client, command, cli.json).await,
        Commands::Providers { command } => providers(client, command, cli.json).await,
        Commands::Wdl { command } => wdl(command, cli.json),
        Commands::Settings { command } => settings(client, command, cli.json).await,
    }
}

async fn status(client: &Client, json_output: bool) -> Result<()> {
    let workflows = match client.fetch_workflows().await {
        Ok(workflows) => workflows,
        Err(err) => {
            if json_output {
                return output::json(&json!({
                    "api": { "reachable": false, "error": err.to_string() }
                }));
            }
            println!("api: unreachable");
            println!("error: {err}");
            return Ok(());
        }
    };
    let supervisor = match client.fetch_supervisor_status().await {
        Ok(value) => value,
        Err(err) => json!({ "configured": false, "error": err.to_string() }),
    };
    let mut counts = Map::new();
    for status in non_terminal_statuses() {
        let runs = client.fetch_workflow_runs_by_status(status).await?;
        counts.insert(status.as_str().into(), runs.len().into());
    }

    if json_output {
        return output::json(&json!({
            "api": { "reachable": true, "workflow_count": workflows.len() },
            "supervisor": supervisor,
            "workflow_runs": counts
        }));
    }

    println!("api: reachable");
    println!("workflows: {}", workflows.len());
    match supervisor.get("configured").and_then(Value::as_bool) {
        Some(true) => {
            let stale = supervisor
                .get("stale_seconds")
                .and_then(Value::as_i64)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into());
            println!("supervisor: configured, stale_seconds={stale}");
        }
        _ => println!("supervisor: unavailable"),
    }
    println!();
    println!("{:<18} {:>6}", "status", "runs");
    for (status, count) in counts {
        println!("{:<18} {:>6}", status, count.as_u64().unwrap_or_default());
    }
    Ok(())
}

async fn workflows(client: &Client, command: &WorkflowCommands, json_output: bool) -> Result<()> {
    match command {
        WorkflowCommands::List => {
            let workflows = client.fetch_workflows().await?;
            if json_output {
                return output::json(&workflows);
            }
            print_workflows(&workflows);
        }
        WorkflowCommands::Show { workflow } => {
            let workflow = fetch_workflow_ref(client, workflow).await?;
            if json_output {
                return output::json(&workflow);
            }
            print_workflow(&workflow)?;
        }
        WorkflowCommands::Validate { file } => {
            let workflow = read_workflow_definition(file)?;
            let workflow = client.validate_workflow(&workflow).await?;
            if json_output {
                return output::json(&workflow);
            }
            println!("workflow {} v{} validates", workflow.name, workflow.version);
        }
        WorkflowCommands::Apply { file } => {
            let value = params::load_json_file(file)?;
            if value.get("workflows").is_some() {
                let bundle: WorkflowBundle = serde_json::from_value(value.into())?;
                let bundle = client.import_workflow_bundle(&bundle).await?;
                if json_output {
                    return output::json(&bundle);
                }
                println!(
                    "imported {} workflows and {} triggers",
                    bundle.workflows.len(),
                    bundle.triggers.len()
                );
            } else {
                let workflow: WorkflowDefinition = serde_json::from_value(value.into())?;
                let workflow = client.upsert_workflow(&workflow).await?;
                if json_output {
                    return output::json(&workflow);
                }
                println!(
                    "saved workflow {} v{} id={}",
                    workflow.name,
                    workflow.version,
                    workflow.id.unwrap_or_default()
                );
            }
        }
        WorkflowCommands::Export {
            workflow_id,
            output: path,
        } => {
            let bundle = client.export_workflow_bundle(*workflow_id).await?;
            if let Some(path) = path {
                write_json_file(path, &bundle)?;
                if !json_output {
                    println!("wrote {}", path.display());
                }
            }
            if json_output || path.is_none() {
                output::json(&bundle)?;
            }
        }
        WorkflowCommands::Run {
            workflow,
            params: cli_params,
            json_file,
            debug,
            name,
        } => {
            let workflow = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = workflow
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let payload = params::load_object(json_file.as_deref(), cli_params)?;
            let run = client
                .create_workflow_run_with_options(workflow_id, payload, *debug, name.clone())
                .await?;
            if json_output {
                return output::json(&run);
            }
            print_run_summary(&run);
        }
    }
    Ok(())
}

fn wdl(command: &WdlCommands, json_output: bool) -> Result<()> {
    let options = runinator_wdl::CompileOptions::default();
    match command {
        WdlCommands::Compile { file, output } => {
            let source = fs::read_to_string(file)?;
            let definition = runinator_wdl::compile_str(&source, &options)
                .map_err(|e| err(e.render(&source)))?;
            if json_output {
                return output::json(&definition);
            }
            let rendered = serde_json::to_string_pretty(&definition)?;
            match output {
                Some(path) => {
                    fs::write(path, rendered)?;
                    println!("wrote {}", path.display());
                }
                None => println!("{rendered}"),
            }
        }
        WdlCommands::Decompile {
            file,
            output,
            explicit,
        } => {
            let definition = read_workflow_definition(file)?;
            let options = runinator_wdl::DecompileOptions {
                explicit: *explicit,
            };
            let source = runinator_wdl::decompile_with(&definition, &options)
                .map_err(|e| err(e.to_string()))?;
            match output {
                Some(path) => {
                    fs::write(path, &source)?;
                    println!("wrote {}", path.display());
                }
                None => print!("{source}"),
            }
        }
        WdlCommands::Format {
            file,
            output,
            check,
        } => {
            let source = fs::read_to_string(file)?;
            let formatted =
                runinator_wdl::format_str(&source).map_err(|e| err(e.render(&source)))?;
            if *check {
                if formatted == source {
                    println!("{} ok", file.display());
                    return Ok(());
                }
                return Err(err(format!("{} is not formatted", file.display())));
            }
            match output {
                Some(path) => {
                    fs::write(path, formatted)?;
                    println!("wrote {}", path.display());
                }
                None => print!("{formatted}"),
            }
        }
        WdlCommands::Check { file } => {
            let source = fs::read_to_string(file)?;
            // analyze first so every error and warning is reported, not just the first.
            let diagnostics =
                runinator_wdl::analyze_source(&source).map_err(|e| err(e.render(&source)))?;
            let error_count = diagnostics.iter().filter(|d| d.is_error()).count();
            if json_output {
                return output::json(&json!({
                    "ok": error_count == 0,
                    "diagnostics": diagnostics
                        .iter()
                        .map(|d| json!({
                            "severity": if d.is_error() { "error" } else { "warning" },
                            "message": d.message,
                            "start": d.span.start,
                            "end": d.span.end,
                        }))
                        .collect::<Vec<_>>(),
                }));
            }
            for diagnostic in &diagnostics {
                eprintln!("{}\n", diagnostic.render(&source));
            }
            if error_count > 0 {
                return Err(err(format!(
                    "{error_count} error(s) found in {}",
                    file.display()
                )));
            }
            // no errors: run the full compile (validator included) for the summary line.
            let definition = runinator_wdl::compile_str(&source, &options)
                .map_err(|e| err(e.render(&source)))?;
            println!("{} v{} ok", definition.name, definition.version);
        }
    }
    Ok(())
}

async fn runs(client: &Client, command: &RunCommands, json_output: bool) -> Result<()> {
    match command {
        RunCommands::List {
            status,
            workflow_id,
            open,
        } => {
            let runs = fetch_runs(client, status.as_deref(), *workflow_id, *open).await?;
            if json_output {
                return output::json(&runs);
            }
            print_runs(&runs);
        }
        RunCommands::Show { id } => {
            let (run, nodes) = client.fetch_workflow_run(*id).await?;
            if json_output {
                return output::json(&json!({ "run": run, "nodes": nodes }));
            }
            print_run_detail(&run, &nodes);
        }
        RunCommands::Watch {
            id,
            interval_seconds,
        } => loop {
            let (run, nodes) = client.fetch_workflow_run(*id).await?;
            if json_output {
                output::json(&json!({ "run": run, "nodes": nodes }))?;
            } else {
                print_run_detail(&run, &nodes);
            }
            if run.status.is_terminal() {
                break;
            }
            time::sleep(Duration::from_secs(*interval_seconds)).await;
            if !json_output {
                println!();
            }
        },
        RunCommands::Logs {
            node_run_id,
            cursor,
            limit,
        } => {
            let chunks = client
                .fetch_workflow_node_run_chunks(*node_run_id, *cursor, *limit)
                .await?;
            if json_output {
                return output::json(&chunks);
            }
            for chunk in chunks {
                print!("{}", chunk.content);
                if !chunk.content.ends_with('\n') {
                    println!();
                }
            }
        }
        RunCommands::Pause { id } => print_task_response(
            client.pause_workflow_run(*id).await?,
            "paused workflow run",
            json_output,
        )?,
        RunCommands::Resume { id } => print_task_response(
            client.resume_workflow_run(*id).await?,
            "resumed workflow run",
            json_output,
        )?,
        RunCommands::Cancel { id } => print_task_response(
            client.cancel_workflow_run(*id).await?,
            "canceled workflow run",
            json_output,
        )?,
        RunCommands::Replay { id, from_step_id } => {
            let run = client
                .replay_workflow_run(*id, from_step_id.clone())
                .await?;
            if json_output {
                return output::json(&run);
            }
            print_run_summary(&run);
        }
        RunCommands::Rename { id, name } => print_task_response(
            client.rename_workflow_run(*id, name.clone()).await?,
            "renamed workflow run",
            json_output,
        )?,
    }
    Ok(())
}

async fn approvals(client: &Client, command: &ApprovalCommands, json_output: bool) -> Result<()> {
    match command {
        ApprovalCommands::List {
            workflow_run_id,
            open,
        } => {
            let mut approvals = client.fetch_approvals(*workflow_run_id).await?;
            if *open {
                approvals.retain(|approval| value_str(approval, "status") == Some("open"));
            }
            if json_output {
                return output::json(&approvals);
            }
            print_approvals(&approvals);
        }
        ApprovalCommands::Approve {
            id,
            by,
            message,
            json_file,
        } => {
            let output_json = optional_json(json_file)?;
            let approval = client
                .approve_request(*id, by.clone(), message.clone(), output_json)
                .await?;
            if json_output {
                return output::json(&approval);
            }
            println!("approved request {id}");
        }
        ApprovalCommands::Reject {
            id,
            by,
            message,
            json_file,
        } => {
            let output_json = optional_json(json_file)?;
            let approval = client
                .reject_request(*id, by.clone(), message.clone(), output_json)
                .await?;
            if json_output {
                return output::json(&approval);
            }
            println!("rejected request {id}");
        }
    }
    Ok(())
}

async fn triggers(client: &Client, command: &TriggerCommands, json_output: bool) -> Result<()> {
    match command {
        TriggerCommands::List { workflow } => {
            let workflow = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = workflow
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let triggers = client.fetch_workflow_triggers(workflow_id).await?;
            if json_output {
                return output::json(&triggers);
            }
            print_triggers(&triggers);
        }
        TriggerCommands::Due => {
            let triggers = client.fetch_due_workflow_triggers().await?;
            if json_output {
                return output::json(&triggers);
            }
            print_triggers(&triggers);
        }
        TriggerCommands::Run {
            trigger_id,
            params: cli_params,
            json_file,
            debug,
        } => {
            let payload = params::load_object(json_file.as_deref(), cli_params)?;
            let run = client
                .create_workflow_trigger_run(*trigger_id, payload, *debug)
                .await?;
            if json_output {
                return output::json(&run);
            }
            print_run_summary(&run);
        }
    }
    Ok(())
}

async fn providers(client: &Client, command: &ProviderCommands, json_output: bool) -> Result<()> {
    match command {
        ProviderCommands::List => {
            let providers = client.fetch_providers().await?;
            if json_output {
                return output::json(&providers);
            }
            print_providers(&providers);
        }
        ProviderCommands::Show { name } => {
            let providers = client.fetch_providers().await?;
            let Some(provider) = providers
                .into_iter()
                .find(|provider| provider.name == *name)
            else {
                return Err(err(format!("provider '{name}' not found")));
            };
            if json_output {
                return output::json(&provider);
            }
            print_provider(&provider);
        }
    }
    Ok(())
}

async fn settings(client: &Client, command: &SettingsCommands, json_output: bool) -> Result<()> {
    match command {
        SettingsCommands::List { kind } => {
            let mut entries = client.list_settings().await?;
            if let Some(kind) = kind {
                let kind = SettingKind::from(*kind);
                entries.retain(|entry| entry.kind == kind);
            }
            if json_output {
                return output::json(&entries);
            }
            print_settings(&entries);
        }
        SettingsCommands::Get { scope, name, kind } => {
            let value = client
                .get_setting(SettingKind::from(*kind), scope, name)
                .await?;
            if json_output {
                return output::json(&value);
            }
            match &value {
                Value::String(text) => println!("{text}"),
                other => println!("{}", serde_json::to_string_pretty(other)?),
            }
        }
        SettingsCommands::Set {
            scope,
            name,
            value,
            value_file,
            kind,
            schema,
        } => {
            let kind = SettingKind::from(*kind);
            let raw = resolve_set_value(value.as_deref(), value_file.as_deref())?;
            // config values are json; secrets are passed through as a plain string.
            let value = match kind {
                SettingKind::Config => serde_json::from_str::<Value>(&raw)
                    .map_err(|e| err(format!("config value must be valid json: {e}")))?,
                SettingKind::Secret => Value::String(raw),
            };
            let schema = match schema {
                Some(text) => Some(
                    serde_json::from_str::<Value>(text)
                        .map_err(|e| err(format!("--schema must be valid json: {e}")))?,
                ),
                None => None,
            };
            let response = client
                .put_setting(kind, scope, name, &value, schema.as_ref())
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!("stored {} {scope}/{name}", kind.as_str());
        }
        SettingsCommands::Import { file } => {
            let value = params::load_json_file(file)?;
            let bundle: SecretBundle = serde_json::from_value(value.into())?;
            let imported = client.import_secret_bundle(&bundle).await?;
            if json_output {
                return output::json(&imported);
            }
            println!(
                "imported {} setting(s) from {}",
                imported.secrets.len(),
                file.display()
            );
        }
        SettingsCommands::Delete { scope, name, kind } => {
            let response = client
                .delete_setting(SettingKind::from(*kind), scope, name)
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!(
                "deleted {} {scope}/{name}",
                SettingKind::from(*kind).as_str()
            );
        }
    }
    Ok(())
}

// resolves a set value from the inline argument or a file, requiring exactly one.
fn resolve_set_value(inline: Option<&str>, file: Option<&Path>) -> Result<String> {
    match (inline, file) {
        (Some(value), None) => Ok(value.to_string()),
        (None, Some(path)) => Ok(fs::read_to_string(path)?),
        (Some(_), Some(_)) => Err(err("provide either VALUE or --value-file, not both")),
        (None, None) => Err(err("a VALUE argument or --value-file is required")),
    }
}

fn print_settings(entries: &[runinator_models::settings::SettingSummary]) {
    println!("{:<8} {:<20} name", "kind", "scope");
    for entry in entries {
        println!(
            "{:<8} {:<20} {}",
            entry.kind.as_str(),
            output::truncate(&entry.scope, 20),
            entry.name
        );
    }
}

async fn fetch_workflow_ref(client: &Client, workflow: &str) -> Result<WorkflowDefinition> {
    if let Ok(id) = workflow.parse::<i64>() {
        return Ok(client.fetch_workflow(id).await?);
    }
    Ok(client.fetch_workflow_by_name(workflow).await?)
}

async fn fetch_runs(
    client: &Client,
    status: Option<&str>,
    workflow_id: Option<i64>,
    open: bool,
) -> Result<Vec<WorkflowRun>> {
    if let Some(status) = status {
        let status = parse_workflow_status(status)?;
        if workflow_id.is_some() {
            let mut runs = client.fetch_workflow_runs(None, workflow_id).await?;
            runs.retain(|run| run.status == status);
            return Ok(runs);
        }
        return client
            .fetch_workflow_runs(Some(status), workflow_id)
            .await
            .map_err(Into::into);
    }

    if open {
        if workflow_id.is_some() {
            let mut runs = client.fetch_workflow_runs(None, workflow_id).await?;
            runs.retain(|run| !run.status.is_terminal());
            return Ok(runs);
        }
        let mut runs = Vec::new();
        for status in non_terminal_statuses() {
            runs.extend(
                client
                    .fetch_workflow_runs(Some(status), workflow_id)
                    .await?,
            );
        }
        runs.sort_by_key(|run| run.id);
        runs.reverse();
        return Ok(runs);
    }

    Ok(client.fetch_workflow_runs(None, workflow_id).await?)
}

fn parse_workflow_status(value: &str) -> Result<WorkflowStatus> {
    WorkflowStatus::try_from(value).map_err(err)
}

fn non_terminal_statuses() -> [WorkflowStatus; 7] {
    [
        WorkflowStatus::Queued,
        WorkflowStatus::Running,
        WorkflowStatus::Paused,
        WorkflowStatus::DebugPaused,
        WorkflowStatus::Waiting,
        WorkflowStatus::ApprovalRequired,
        WorkflowStatus::Blocked,
    ]
}

fn read_workflow_definition(path: &Path) -> Result<WorkflowDefinition> {
    let value = params::load_json_file(path)?;
    Ok(serde_json::from_value(value.into())?)
}

fn write_json_file<T: serde::Serialize>(path: &PathBuf, value: &T) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn optional_json(path: &Option<PathBuf>) -> Result<Option<Value>> {
    path.as_deref().map(params::load_json_file).transpose()
}

fn print_workflows(workflows: &[WorkflowDefinition]) {
    println!(
        "{:<6} {:<36} {:>7} {:<8} updated_at",
        "id", "name", "version", "enabled"
    );
    for workflow in workflows {
        println!(
            "{:<6} {:<36} {:>7} {:<8} {}",
            workflow.id.unwrap_or_default(),
            output::truncate(&workflow.name, 36),
            workflow.version,
            workflow.enabled,
            output::time(workflow.updated_at)
        );
    }
}

fn print_workflow(workflow: &WorkflowDefinition) -> Result<()> {
    println!("id: {}", workflow.id.unwrap_or_default());
    println!("name: {}", workflow.name);
    println!("version: {}", workflow.version);
    println!("enabled: {}", workflow.enabled);
    println!("updated_at: {}", output::time(workflow.updated_at));
    println!(
        "definition: {}",
        serde_json::to_string_pretty(&workflow.definition)?
    );
    Ok(())
}

fn print_runs(runs: &[WorkflowRun]) {
    println!(
        "{:<6} {:<18} {:<10} {:<22} {:<18} message",
        "id", "status", "workflow", "active_node", "created_at"
    );
    for run in runs {
        println!(
            "{:<6} {:<18} {:<10} {:<22} {:<18} {}",
            run.id,
            run.status.as_str(),
            run.workflow_id,
            output::truncate(run.active_node_id.as_deref().unwrap_or("-"), 22),
            output::truncate(&run.created_at.to_rfc3339(), 18),
            output::truncate(run.message.as_deref().unwrap_or(""), 48)
        );
    }
}

fn print_run_summary(run: &WorkflowRun) {
    println!(
        "workflow_run id={} workflow_id={} status={} active_node={}",
        run.id,
        run.workflow_id,
        run.status.as_str(),
        run.active_node_id.as_deref().unwrap_or("-")
    );
}

fn print_run_detail(run: &WorkflowRun, nodes: &[WorkflowNodeRun]) {
    print_run_summary(run);
    println!("created_at: {}", run.created_at.to_rfc3339());
    println!("started_at: {}", output::time(run.started_at));
    println!("finished_at: {}", output::time(run.finished_at));
    if let Some(message) = &run.message {
        println!("message: {message}");
    }
    println!();
    println!(
        "{:<6} {:<28} {:<18} {:>7} message",
        "id", "node_id", "status", "attempt"
    );
    for node in nodes {
        println!(
            "{:<6} {:<28} {:<18} {:>7} {}",
            node.id,
            output::truncate(&node.node_id, 28),
            node.status.as_str(),
            node.attempt,
            output::truncate(node.message.as_deref().unwrap_or(""), 48)
        );
    }
}

fn print_task_response<T: serde::Serialize>(
    response: T,
    message: &str,
    json_output: bool,
) -> Result<()> {
    if json_output {
        return output::json(&response);
    }
    println!("{message}");
    Ok(())
}

fn print_approvals(approvals: &[Value]) {
    println!(
        "{:<6} {:<18} {:<10} {:<24} prompt",
        "id", "status", "run", "node"
    );
    for approval in approvals {
        println!(
            "{:<6} {:<18} {:<10} {:<24} {}",
            value_i64(approval, "id").unwrap_or_default(),
            value_str(approval, "status").unwrap_or("-"),
            value_i64(approval, "workflow_run_id").unwrap_or_default(),
            output::truncate(value_str(approval, "node_id").unwrap_or("-"), 24),
            output::truncate(value_str(approval, "prompt").unwrap_or(""), 64)
        );
    }
}

fn print_triggers(triggers: &[WorkflowTrigger]) {
    println!(
        "{:<6} {:<10} {:<8} {:<10} next_execution",
        "id", "workflow", "enabled", "kind"
    );
    for trigger in triggers {
        println!(
            "{:<6} {:<10} {:<8} {:<10} {}",
            trigger.id.unwrap_or_default(),
            trigger.workflow_id,
            trigger.enabled,
            trigger.kind.as_str(),
            output::time(trigger.next_execution)
        );
    }
}

fn print_providers(providers: &[ProviderMetadata]) {
    println!("{:<28} {:>7} credential_scopes", "name", "actions");
    for provider in providers {
        println!(
            "{:<28} {:>7} {}",
            output::truncate(&provider.name, 28),
            provider.actions.len(),
            provider.metadata.credential_scopes.join(",")
        );
    }
}

fn print_provider(provider: &ProviderMetadata) {
    println!("name: {}", provider.name);
    if !provider.metadata.credential_scopes.is_empty() {
        println!(
            "credential_scopes: {}",
            provider.metadata.credential_scopes.join(",")
        );
    }
    println!();
    println!("{:<32} parameters", "action");
    for action in &provider.actions {
        let parameters = action
            .parameters
            .iter()
            .map(|param| {
                if param.required {
                    format!("{}*", param.name)
                } else {
                    param.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:<32} {}",
            output::truncate(&action.function_name, 32),
            parameters
        );
        if let Some(description) = &action.description {
            println!("  {}", output::truncate(description, 96));
        }
    }
}

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn value_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}
