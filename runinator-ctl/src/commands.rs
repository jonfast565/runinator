use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

use chrono::Utc;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::{
    billing::ScaleOrgNodesRequest,
    providers::ProviderMetadata,
    provisioning::{NodeSpec, ProvisionedGroup, ScaleNodesRequest, StopNodeRequest},
    replicas::ReplicaKind,
    revisions::WorkflowRevision,
    schedules::{BackfillRequest, FreezeWindow, NewFreezeWindow},
    settings::SettingKind,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowRun, WorkflowStatus, WorkflowTrigger},
};
use tokio::time;

use runinator_pack::source as pack;

use crate::{output, params};
use runinator_ctl_core::cli::{
    AgentCommands, ApprovalCommands, ArtifactCommands, Cli, Commands, FreezeCommands, NodeCommands,
    OrgCommands, ProviderCommands, RexRapCommands, RunCommands, SettingsCommands, TriggerCommands,
    WorkflowCommands,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

type Client = AsyncApiClient<StaticLocator>;

#[derive(Debug, Clone)]
struct WorkflowApplySummary {
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSnapshot {
    files: Vec<SourceFileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceFileSnapshot {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: Option<u64>,
}

pub fn err(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

pub async fn run(client: &Client, cli: &Cli) -> Result<()> {
    run_command(client, &cli.command, &cli.api_base_url, cli.json).await
}

/// dispatch one command.
///
/// separate from `run` so the console repl reaches the same commands the command line does: it
/// parses a `:`-prefixed line into a `Commands` and lands here, rather than keeping a second,
/// smaller table of verbs that would drift.
pub async fn run_command(
    client: &Client,
    command: &Commands,
    api_base_url: &str,
    json_output: bool,
) -> Result<()> {
    match command {
        // login/logout are intercepted in main before dispatch; reaching here means that wiring
        // changed, so report it instead of panicking.
        Commands::Login | Commands::Logout => Err(err(
            "login and logout must be handled before command dispatch",
        )),
        Commands::Status => status::status(client, json_output).await,
        Commands::Workflows { command } => workflows::workflows(client, command, json_output).await,
        Commands::Runs { command } => runs::runs(client, command, json_output).await,
        Commands::Approvals { command } => approvals::approvals(client, command, json_output).await,
        Commands::Triggers { command } => triggers::triggers(client, command, json_output).await,
        Commands::Freeze { command } => freeze::freeze(client, command, json_output).await,
        Commands::Providers { command } => providers::providers(client, command, json_output).await,
        Commands::Functions { command } => functions::functions(client, command, json_output).await,
        Commands::Pipelines { command } => pipelines::pipelines(client, command, json_output).await,
        Commands::Orchestrations { command } => {
            orchestrations::orchestrations(client, command, json_output).await
        }
        Commands::Mcp {
            workflow_tools,
            timeout,
        } => {
            mcp::serve(
                client,
                api_base_url,
                mcp::Options {
                    workflow_tools: *workflow_tools,
                    timeout: Duration::from_secs(*timeout),
                },
            )
            .await
        }
        Commands::Console {
            session,
            new_session,
            execute,
            file,
            no_follow,
            plain,
        } => {
            console::console(
                client,
                session.as_deref(),
                new_session.as_deref(),
                execute.as_deref(),
                file.as_deref(),
                *no_follow,
                json_output,
                api_base_url,
                *plain,
            )
            .await
        }
        Commands::Artifacts { command } => artifacts::artifacts(client, command, json_output).await,
        Commands::RexRap { command } => workflows::rexrap(command, json_output),
        Commands::Settings { command } => settings::settings(client, command, json_output).await,
        Commands::Namespaces { command } => {
            namespaces::namespaces(client, command, json_output).await
        }
        Commands::Nodes { command } => nodes::nodes(client, command, json_output).await,
        Commands::Orgs { command } => orgs::orgs(client, command, json_output).await,
        Commands::Replicas { command } => replicas::replicas(client, command, json_output).await,
        Commands::Agents { command } => {
            agents::agents(client, command, api_base_url, json_output).await
        }
    }
}

mod agents;
mod namespaces;
mod nodes;
mod orgs;
mod status;
mod workflows;
pub use workflows::workflows_test;
mod approvals;
mod artifacts;
mod freeze;
mod functions;
pub use functions::functions_validate;
mod console;
mod mcp;
mod orchestrations;
mod pipelines;
mod providers;
pub(crate) mod repl;
mod repl_completer;
mod replicas;
mod runs;
mod settings;
mod timeline;
mod triggers;

async fn fetch_workflow_ref(client: &Client, workflow: &str) -> Result<WorkflowDefinition> {
    if let Ok(id) = workflow.parse::<Uuid>() {
        return Ok(client.fetch_workflow(id).await?);
    }
    Ok(client.fetch_workflow_by_name(workflow).await?)
}

async fn fetch_runs(
    client: &Client,
    status: Option<&str>,
    workflow_id: Option<Uuid>,
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
        runs.sort_by_key(|run| run.created_at);
        runs.reverse();
        return Ok(runs);
    }

    Ok(client.fetch_workflow_runs(None, workflow_id).await?)
}

fn parse_workflow_status(value: &str) -> Result<WorkflowStatus> {
    WorkflowStatus::try_from(value).map_err(err)
}

fn non_terminal_statuses() -> [WorkflowStatus; 10] {
    [
        WorkflowStatus::Queued,
        WorkflowStatus::Running,
        WorkflowStatus::Paused,
        WorkflowStatus::DebugPaused,
        WorkflowStatus::Waiting,
        WorkflowStatus::Parked,
        WorkflowStatus::Sleeping,
        WorkflowStatus::ApprovalRequired,
        WorkflowStatus::InputRequired,
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

fn print_workflow_revisions(revisions: &[WorkflowRevision]) {
    println!(
        "{:<4} {:<7} {:<10} {:<38} {:<28} created_at",
        "rev", "version", "source", "name", "author"
    );
    for revision in revisions {
        println!(
            "{:<4} {:<7} {:<10} {:<38} {:<28} {}",
            revision.revision,
            revision.version,
            revision.source,
            output::truncate(&revision.name, 38),
            output::truncate(&revision_author_label(revision), 28),
            output::time(revision.created_at)
        );
    }
}

// An unattributed write shows its kind rather than a blank shaped like a UUID.
fn revision_author_label(revision: &WorkflowRevision) -> String {
    match revision.actor_id {
        Some(id) => format!("{} {id}", revision.actor_kind),
        None => revision.actor_kind.clone(),
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
            value_display(approval, "id"),
            value_str(approval, "status").unwrap_or("-"),
            value_display(approval, "workflow_run_id"),
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

fn value_display(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => "-".into(),
    }
}
