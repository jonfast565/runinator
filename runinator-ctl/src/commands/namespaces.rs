use super::*;

use runinator_models::artifacts::ArtifactKind;
use runinator_models::settings::SettingKind;
use serde::{Deserialize, Serialize};

use crate::cli::NamespaceCommands;

const PLAN_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NamespaceMigrationPlan {
    version: u32,
    artifacts: Vec<NamespaceMigrationEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    source_diffs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NamespaceMigrationEntry {
    kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    setting_kind: Option<SettingKind>,
    id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    key: String,
    display_name: String,
    current_path: String,
}

pub(super) async fn namespaces(
    client: &Client,
    command: &NamespaceCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        NamespaceCommands::Plan { output } => plan(client, output.as_ref(), json_output).await,
        NamespaceCommands::Apply { file } => apply(client, file, json_output).await,
    }
}

async fn plan(client: &Client, output_path: Option<&PathBuf>, json_output: bool) -> Result<()> {
    let (workflows, pipelines, functions, settings) = tokio::try_join!(
        client.fetch_workflows(),
        client.fetch_pipelines(),
        client.fetch_function_packages(),
        client.list_settings(),
    )?;
    let diagnostics = ambiguous_reference_diagnostics(&workflows);
    let mut source_diffs = Vec::new();
    let mut artifacts = Vec::new();
    for workflow in workflows {
        let Some(id) = workflow.id else { continue };
        let path = workflow.artifact_path();
        artifacts.push(NamespaceMigrationEntry {
            kind: ArtifactKind::Workflow,
            setting_kind: None,
            id,
            namespace: path.namespace.clone(),
            key: path.key.clone(),
            display_name: workflow.name,
            current_path: path.qualified(),
        });
        source_diffs.push(format!(
            "--- workflow {id}\n+++ workflow {id}\n+ namespace {}\n+ key {}",
            path.namespace.as_deref().unwrap_or("<root>"),
            path.key
        ));
    }
    for pipeline in pipelines {
        let Some(id) = pipeline.id else { continue };
        let path = pipeline.artifact_path();
        artifacts.push(NamespaceMigrationEntry {
            kind: ArtifactKind::Pipeline,
            setting_kind: None,
            id,
            namespace: path.namespace.clone(),
            key: path.key.clone(),
            display_name: pipeline.name,
            current_path: path.qualified(),
        });
    }
    for package in functions {
        artifacts.push(NamespaceMigrationEntry {
            kind: ArtifactKind::FunctionPackage,
            setting_kind: None,
            id: package.id,
            namespace: package.namespace.clone(),
            key: package.name.clone(),
            display_name: package.name.clone(),
            current_path: package.qualified_name(),
        });
    }
    for setting in settings {
        let current_path = format!("{}.{}", setting.scope, setting.name);
        artifacts.push(NamespaceMigrationEntry {
            kind: ArtifactKind::Setting,
            setting_kind: Some(setting.kind),
            id: setting.id,
            namespace: Some(setting.scope.clone()),
            key: setting.name.clone(),
            display_name: setting.name,
            current_path,
        });
    }
    artifacts.sort_by(|left, right| {
        format!("{:?}:{}", left.kind, left.current_path)
            .cmp(&format!("{:?}:{}", right.kind, right.current_path))
    });
    let plan = NamespaceMigrationPlan {
        version: PLAN_VERSION,
        artifacts,
        diagnostics,
        source_diffs,
    };
    if let Some(path) = output_path {
        write_json_file(path, &plan)?;
        if !json_output {
            println!("wrote namespace migration plan to {}", path.display());
        }
        return Ok(());
    }
    output::json(&plan)
}

async fn apply(client: &Client, file: &Path, json_output: bool) -> Result<()> {
    let plan: NamespaceMigrationPlan =
        serde_json::from_value(params::load_json_file(file)?.into())?;
    if plan.version != PLAN_VERSION {
        return Err(err(format!(
            "unsupported namespace plan version {}; expected {PLAN_VERSION}",
            plan.version
        )));
    }
    if !plan.diagnostics.is_empty() {
        return Err(err(format!(
            "namespace plan contains {} unresolved diagnostic(s); resolve them before apply",
            plan.diagnostics.len()
        )));
    }
    ensure_unique_paths(&plan)?;
    ensure_runs_drained(client).await?;

    let mut workflows_updated = 0usize;
    let mut pipelines_updated = 0usize;
    let mut settings_moved = 0usize;
    let mut function_packages_moved = 0usize;
    for entry in &plan.artifacts {
        if entry.key.trim().is_empty() {
            return Err(err(format!("{} has an empty stable key", entry.id)));
        }
        let namespace = strict_namespace(entry)?;
        match entry.kind {
            ArtifactKind::Workflow => {
                let mut workflow = client.fetch_workflow(entry.id).await?;
                workflow.key = Some(entry.key.clone());
                workflow.namespace = Some(namespace.clone());
                workflow.name = entry.display_name.clone();
                client.upsert_workflow(&workflow).await?;
                workflows_updated += 1;
            }
            ArtifactKind::Pipeline => {
                let mut pipeline = client.fetch_pipeline(entry.id).await?;
                pipeline.key = Some(entry.key.clone());
                pipeline.namespace = Some(namespace.clone());
                pipeline.name = entry.display_name.clone();
                client.upsert_pipeline(&pipeline).await?;
                pipelines_updated += 1;
            }
            // These resources already have durable UUIDs. Their current APIs do not expose an
            // identity-move operation, so an edited move is rejected instead of silently ignored.
            ArtifactKind::Setting => {
                let requested = qualified(Some(&namespace), &entry.key);
                if requested != entry.current_path {
                    let kind = entry
                        .setting_kind
                        .ok_or_else(|| err(format!("setting {} has no setting_kind", entry.id)))?;
                    client
                        .move_setting(entry.id, kind, &namespace, &entry.key)
                        .await?;
                    settings_moved += 1;
                }
            }
            ArtifactKind::FunctionPackage => {
                let requested = qualified(Some(&namespace), &entry.key);
                if requested != entry.current_path {
                    client
                        .move_function_package(entry.id, Some(&namespace), &entry.key)
                        .await?;
                    function_packages_moved += 1;
                }
            }
        }
    }
    let result = json!({
        "workflows_updated": workflows_updated,
        "pipelines_updated": pipelines_updated,
        "settings_moved": settings_moved,
        "function_packages_moved": function_packages_moved,
        "strict_namespace_mode": true,
    });
    if json_output {
        return output::json(&result);
    }
    println!(
        "updated {workflows_updated} workflow(s) and {pipelines_updated} pipeline(s); moved {settings_moved} setting(s) and {function_packages_moved} function package(s); UUID references were revalidated"
    );
    println!("strict namespace enforcement is active");
    Ok(())
}

async fn ensure_runs_drained(client: &Client) -> Result<()> {
    let workflow_runs = fetch_runs(client, None, None, true).await?;
    let pipeline_runs = client.fetch_pipeline_runs(None).await?;
    let active_pipelines = pipeline_runs
        .iter()
        .filter(|run| !run.status.is_terminal())
        .count();
    if !workflow_runs.is_empty() || active_pipelines > 0 {
        return Err(err(format!(
            "namespace apply requires runs to drain ({} workflow run(s), {} pipeline run(s) still active)",
            workflow_runs.len(),
            active_pipelines
        )));
    }
    Ok(())
}

fn ensure_unique_paths(plan: &NamespaceMigrationPlan) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for entry in &plan.artifacts {
        let namespace = strict_namespace(entry)?;
        let path = qualified(Some(&namespace), &entry.key);
        if !seen.insert((entry.kind, entry.setting_kind, path.clone())) {
            return Err(err(format!("duplicate {:?} path '{path}'", entry.kind)));
        }
    }
    Ok(())
}

fn strict_namespace(entry: &NamespaceMigrationEntry) -> Result<String> {
    let namespace = entry
        .namespace
        .as_deref()
        .map(str::trim)
        .filter(|namespace| {
            !namespace.is_empty()
                && namespace.split('.').all(|segment| {
                    let mut chars = segment.chars();
                    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
                        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                })
        })
        .ok_or_else(|| {
            err(format!(
                "{:?} '{}' must declare an explicit namespace before strict apply",
                entry.kind, entry.display_name
            ))
        })?;
    Ok(namespace.to_string())
}

fn ambiguous_reference_diagnostics(
    workflows: &[runinator_models::workflows::WorkflowDefinition],
) -> Vec<String> {
    let mut aliases: std::collections::HashMap<String, Vec<Uuid>> =
        std::collections::HashMap::new();
    for workflow in workflows {
        let Some(id) = workflow.id else { continue };
        for alias in [workflow.artifact_path().qualified(), workflow.name.clone()] {
            let ids = aliases.entry(alias).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    let mut diagnostics = Vec::new();
    for workflow in workflows {
        for node in &workflow.definition.nodes {
            if node.kind != runinator_models::workflows::WorkflowNodeKind::Subflow
                || node.subflow.target_workflow_id().is_some()
            {
                continue;
            }
            let Some(path) = node.subflow.authored_path() else {
                diagnostics.push(format!(
                    "workflow '{}' node '{}' has no resolvable subflow path",
                    workflow.name, node.id
                ));
                continue;
            };
            let candidates = aliases
                .get(&path.qualified())
                .map(Vec::as_slice)
                .unwrap_or_default();
            if candidates.len() != 1 {
                diagnostics.push(format!(
                    "workflow '{}' node '{}' references '{}', which resolves to {} UUIDs",
                    workflow.name,
                    node.id,
                    path,
                    candidates.len()
                ));
            }
        }
    }
    diagnostics
}

fn qualified(namespace: Option<&str>, key: &str) -> String {
    match namespace.map(str::trim).filter(|value| !value.is_empty()) {
        Some(namespace) => format!("{namespace}.{key}"),
        None => key.to_string(),
    }
}
