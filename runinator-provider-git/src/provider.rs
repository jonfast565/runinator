use std::{fs, path::Path, sync::Arc};

use runinator_models::json;
use runinator_models::{
    errors::SendableError,
    orchestration::DeliverySemantics,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{NewRunArtifact, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::Serialize;

use crate::command::{run_command, run_command_output};
use crate::errors::{IO_ERROR, REVISION_MISMATCH, UNSUPPORTED_ACTION, WORKSPACE_SAFETY};
use crate::params::{
    ArchivePatchParams, AttemptWorktreeParams, CleanupParams, CommitParams, PromoteRevisionParams,
    PushParams, WorkspaceParams, WorktreeParams, parse_params,
};

#[derive(Serialize)]
struct GitResult {
    stdout: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
}

#[derive(Clone)]
pub struct GitProvider;

impl Provider for GitProvider {
    fn name(&self) -> String {
        "git".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("worktree", "Manage git worktrees")
                    .with_parameters(vec![
                        ParameterMetadata::optional("repo", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("branch", RuninatorType::String),
                        ParameterMetadata::required("path", RuninatorType::String),
                    ])
                    .with_results(worktree_results()),
                ActionMetadata::new(
                    "attempt_worktree",
                    "Create or reconcile the current attempt's fenced worktree",
                )
                .with_parameters(vec![
                    ParameterMetadata::optional("repo", RuninatorType::String)
                        .with_default(json!(".")),
                    ParameterMetadata::required("branch", RuninatorType::String),
                    ParameterMetadata::optional("path", RuninatorType::String),
                    ParameterMetadata::optional("base_ref", RuninatorType::String),
                ])
                .with_results(worktree_results())
                .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new("branch", "Get current branch name")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                    ])
                    .with_results(git_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("commit", "Add and commit all changes")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("message", RuninatorType::String),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("push", "Push a branch to a remote")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::optional("remote", RuninatorType::String)
                            .with_default(json!("origin")),
                        ParameterMetadata::required("branch", RuninatorType::String),
                        ParameterMetadata::optional("set_upstream", RuninatorType::Boolean)
                            .with_default(json!(true)),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("diff", "Get git diff summary")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                    ])
                    .with_results(git_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new(
                    "capture_revision",
                    "Capture the exact revision and dirty state of the fenced workspace",
                )
                .with_parameters(vec![
                    ParameterMetadata::optional("workspace", RuninatorType::String)
                        .with_default(json!(".")),
                ])
                .with_results(revision_results())
                .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new(
                    "archive_patch",
                    "Capture the workspace patch and current revision as evidence",
                )
                .with_parameters(vec![
                    ParameterMetadata::optional("workspace", RuninatorType::String)
                        .with_default(json!(".")),
                    ParameterMetadata::optional("name", RuninatorType::String),
                ])
                .with_results(archive_results())
                .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new(
                    "promote_revision",
                    "Guard and promote the fenced workspace revision to a branch ref",
                )
                .with_parameters(vec![
                    ParameterMetadata::optional("workspace", RuninatorType::String),
                    ParameterMetadata::optional("repo", RuninatorType::String),
                    ParameterMetadata::required("candidate_sha", RuninatorType::String),
                    ParameterMetadata::required("target_ref", RuninatorType::String),
                    ParameterMetadata::optional("expected_target_sha", RuninatorType::String),
                    ParameterMetadata::optional("remote", RuninatorType::String)
                        .with_default(json!("origin")),
                    ParameterMetadata::optional("push", RuninatorType::Boolean)
                        .with_default(json!(false)),
                ])
                .with_results(promotion_results())
                .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new("cleanup", "Remove git worktree")
                    .with_parameters(vec![
                        ParameterMetadata::optional("repo", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("path", RuninatorType::String),
                    ])
                    .with_results(git_results())
                    .with_delivery_semantics(DeliverySemantics::Reconcilable),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let function = request.action_function.as_str();
        let timeout = request.timeout_secs;
        let stdout = match function {
            "create_or_resume_worktree" | "worktree" => {
                let params: WorktreeParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                let stdout = run_command(
                    "git",
                    &[
                        "-C",
                        repo,
                        "worktree",
                        "add",
                        "-B",
                        &params.branch,
                        &params.path,
                    ],
                    timeout,
                    &token,
                )?;
                let result = GitResult {
                    stdout,
                    action: function.to_string(),
                    workspace: Some(params.path),
                };
                return Ok(TaskExecutionResult {
                    message: Some(format!("Git action {function} completed")),
                    output_json: serde_json::to_value(result).ok().map(Into::into),
                    chunks: Vec::new(),
                    artifacts: Vec::new(),
                });
            }
            "attempt_worktree" => {
                let params: AttemptWorktreeParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                let path = request
                    .workspace_path
                    .as_deref()
                    .or(params.path.as_deref())
                    .ok_or_else(|| {
                        WORKSPACE_SAFETY
                            .error("attempt_worktree requires a current workspace affinity or path")
                    })?;
                let workspace = Path::new(path);
                if workspace.exists() {
                    let mut entries = fs::read_dir(workspace).map_err(|error| {
                        IO_ERROR.error(format!("could not inspect {path}: {error}"))
                    })?;
                    if entries.next().is_none() {
                        fs::remove_dir(workspace).map_err(|error| {
                            IO_ERROR
                                .error(format!("could not prepare empty workspace {path}: {error}"))
                        })?;
                    } else {
                        let current = run_command_output(
                            "git",
                            &["-C", path, "branch", "--show-current"],
                            timeout,
                            &token,
                        )?;
                        if !current.success || current.stdout.trim() != params.branch {
                            return Err(WORKSPACE_SAFETY.error(format!(
                                "existing workspace {path} is not branch '{}'",
                                params.branch
                            )));
                        }
                        return git_result(
                            function,
                            "worktree already matched the requested branch".into(),
                            Some(path.to_string()),
                        );
                    }
                }
                let mut args = vec!["-C", repo, "worktree", "add", "-B", &params.branch, path];
                if let Some(base_ref) = params.base_ref.as_deref() {
                    args.push(base_ref);
                }
                let stdout = run_command("git", &args, timeout, &token)?;
                return git_result(function, stdout, Some(path.to_string()));
            }
            "branch" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = params
                    .workspace
                    .as_deref()
                    .or(params.repo.as_deref())
                    .unwrap_or(".");
                run_command(
                    "git",
                    &["-C", ws, "branch", "--show-current"],
                    timeout,
                    &token,
                )?
            }
            "commit" => {
                let params: CommitParams = parse_params(&request)?;
                let ws = params.workspace.as_deref().unwrap_or(".");
                run_command("git", &["-C", ws, "add", "."], timeout, &token)?;
                run_command(
                    "git",
                    &["-C", ws, "commit", "-m", &params.message],
                    timeout,
                    &token,
                )?
            }
            "push" => {
                let params: PushParams = parse_params(&request)?;
                let ws = params.workspace.as_deref().unwrap_or(".");
                let remote = params.remote.as_deref().unwrap_or("origin");
                if params.set_upstream.unwrap_or(true) {
                    run_command(
                        "git",
                        &["-C", ws, "push", "-u", remote, &params.branch],
                        timeout,
                        &token,
                    )?
                } else {
                    run_command(
                        "git",
                        &["-C", ws, "push", remote, &params.branch],
                        timeout,
                        &token,
                    )?
                }
            }
            "diff" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = params
                    .workspace
                    .as_deref()
                    .or(params.repo.as_deref())
                    .unwrap_or(".");
                run_command("git", &["-C", ws, "diff", "--stat"], timeout, &token)?
            }
            "capture_revision" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = workspace_path(
                    &request,
                    params.workspace.as_deref().or(params.repo.as_deref()),
                );
                return capture_revision(function, ws, timeout, &token);
            }
            "archive_patch" => {
                let params: ArchivePatchParams = parse_params(&request)?;
                let ws = workspace_path(&request, params.workspace.as_deref());
                let sha = run_command("git", &["-C", ws, "rev-parse", "HEAD"], timeout, &token)?;
                let patch = run_command(
                    "git",
                    &["-C", ws, "diff", "--binary", "HEAD"],
                    timeout,
                    &token,
                )?;
                let name =
                    sanitize_artifact_name(params.name.as_deref().unwrap_or("candidate.patch"));
                let artifact_dir = Path::new(&request.artifact_dir);
                fs::create_dir_all(artifact_dir).map_err(|error| {
                    IO_ERROR.error(format!(
                        "could not create artifact directory {}: {error}",
                        artifact_dir.display()
                    ))
                })?;
                let path = artifact_dir.join(name);
                fs::write(&path, patch.as_bytes()).map_err(|error| {
                    IO_ERROR.error(format!("could not write {}: {error}", path.display()))
                })?;
                let status =
                    run_command("git", &["-C", ws, "status", "--porcelain"], timeout, &token)?;
                let output = json!({
                    "sha": sha.trim(),
                    "dirty": !status.trim().is_empty(),
                    "patch_path": path.to_string_lossy(),
                    "size_bytes": patch.len(),
                });
                return Ok(TaskExecutionResult {
                    message: Some("git patch archived".into()),
                    output_json: Some(output),
                    chunks: Vec::new(),
                    artifacts: vec![NewRunArtifact {
                        name: path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("candidate.patch")
                            .into(),
                        mime_type: "text/x-diff".into(),
                        size_bytes: patch.len() as i64,
                        uri: path.to_string_lossy().into_owned(),
                        metadata: json!({ "provider": "git", "sha": sha.trim() }),
                    }],
                });
            }
            "promote_revision" => {
                let params: PromoteRevisionParams = parse_params(&request)?;
                let ws = workspace_path(&request, params.workspace.as_deref());
                return promote_revision(&params, ws, timeout, &token);
            }
            "cleanup" => {
                let params: CleanupParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                if !Path::new(&params.path).exists() {
                    return git_result(
                        function,
                        "worktree already absent".into(),
                        Some(params.path),
                    );
                }
                run_command(
                    "git",
                    &["-C", repo, "worktree", "remove", &params.path],
                    timeout,
                    &token,
                )?
            }
            other => {
                return Err(UNSUPPORTED_ACTION.error(other));
            }
        };
        let result = GitResult {
            stdout,
            action: function.to_string(),
            workspace: None,
        };
        Ok(TaskExecutionResult {
            message: Some(format!("Git action {function} completed")),
            output_json: serde_json::to_value(result).ok().map(Into::into),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn workspace_path<'a>(
    request: &'a ProviderExecutionRequest,
    configured: Option<&'a str>,
) -> &'a str {
    request
        .workspace_path
        .as_deref()
        .or(configured)
        .unwrap_or(".")
}

fn git_result(
    action: &str,
    stdout: String,
    workspace: Option<String>,
) -> Result<TaskExecutionResult, SendableError> {
    let result = GitResult {
        stdout,
        action: action.to_string(),
        workspace,
    };
    Ok(TaskExecutionResult {
        message: Some(format!("Git action {action} completed")),
        output_json: serde_json::to_value(result).ok().map(Into::into),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn capture_revision(
    action: &str,
    workspace: &str,
    timeout: i64,
    token: &runinator_plugin::cancel::CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let sha = run_command(
        "git",
        &["-C", workspace, "rev-parse", "HEAD"],
        timeout,
        token,
    )?;
    let branch = run_command(
        "git",
        &["-C", workspace, "branch", "--show-current"],
        timeout,
        token,
    )?;
    let status = run_command(
        "git",
        &["-C", workspace, "status", "--porcelain"],
        timeout,
        token,
    )?;
    Ok(TaskExecutionResult {
        message: Some("git revision captured".into()),
        output_json: Some(json!({
            "sha": sha.trim(),
            "branch": branch.trim(),
            "dirty": !status.trim().is_empty(),
            "action": action,
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn promote_revision(
    params: &PromoteRevisionParams,
    workspace: &str,
    timeout: i64,
    token: &runinator_plugin::cancel::CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    if !params.target_ref.starts_with("refs/heads/") {
        return Err(WORKSPACE_SAFETY.error("target_ref must be beneath refs/heads/"));
    }
    let repo = params.repo.as_deref().unwrap_or(workspace);
    let candidate_expression = format!("{}^{{commit}}", params.candidate_sha);
    let candidate = run_command(
        "git",
        &["-C", repo, "rev-parse", "--verify", &candidate_expression],
        timeout,
        token,
    )?;
    let candidate = candidate.trim();
    let workspace_head = run_command(
        "git",
        &["-C", workspace, "rev-parse", "HEAD"],
        timeout,
        token,
    )?;
    if workspace_head.trim() != candidate {
        return Err(REVISION_MISMATCH.error(format!(
            "workspace HEAD {} does not match candidate {candidate}",
            workspace_head.trim()
        )));
    }

    let current = run_command_output(
        "git",
        &["-C", repo, "rev-parse", "--verify", &params.target_ref],
        timeout,
        token,
    )?;
    let current_sha = current.success.then(|| current.stdout.trim().to_string());
    if current_sha.as_deref() != Some(candidate) {
        if let Some(expected) = params.expected_target_sha.as_deref()
            && current_sha.as_deref() != Some(expected)
        {
            return Err(REVISION_MISMATCH.error(format!(
                "target {} is {}, expected {expected}",
                params.target_ref,
                current_sha.as_deref().unwrap_or("absent")
            )));
        }
        let mut args = vec!["-C", repo, "update-ref", &params.target_ref, candidate];
        if let Some(expected) = params.expected_target_sha.as_deref() {
            args.push(expected);
        }
        run_command("git", &args, timeout, token)?;
    }

    let mut pushed = false;
    if params.push.unwrap_or(false) {
        let remote = params.remote.as_deref().unwrap_or("origin");
        let remote_ref = run_command(
            "git",
            &["-C", repo, "ls-remote", remote, &params.target_ref],
            timeout,
            token,
        )?;
        let remote_sha = remote_ref.split_whitespace().next();
        if remote_sha != Some(candidate) {
            if let Some(expected) = params.expected_target_sha.as_deref()
                && remote_sha != Some(expected)
            {
                return Err(REVISION_MISMATCH.error(format!(
                    "remote target {} is {}, expected {expected}",
                    params.target_ref,
                    remote_sha.unwrap_or("absent")
                )));
            }
            let refspec = format!("{candidate}:{}", params.target_ref);
            if let Some(expected) = params.expected_target_sha.as_deref() {
                let lease = format!("--force-with-lease={}:{}", params.target_ref, expected);
                run_command(
                    "git",
                    &["-C", repo, "push", &lease, remote, &refspec],
                    timeout,
                    token,
                )?;
            } else {
                run_command(
                    "git",
                    &["-C", repo, "push", remote, &refspec],
                    timeout,
                    token,
                )?;
            }
            pushed = true;
        }
    }
    Ok(TaskExecutionResult {
        message: Some("git revision promoted".into()),
        output_json: Some(json!({
            "candidate_sha": candidate,
            "target_ref": params.target_ref,
            "previous_target_sha": current_sha,
            "pushed": pushed,
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn sanitize_artifact_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim_matches(['.', '_']).is_empty() {
        "candidate.patch".into()
    } else {
        sanitized
    }
}

fn git_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("stdout", RuninatorType::String),
        ResultMetadata::new("action", RuninatorType::String),
    ]
}

fn worktree_results() -> Vec<ResultMetadata> {
    let mut results = git_results();
    results.push(ResultMetadata::new("workspace", RuninatorType::String));
    results
}

fn revision_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("sha", RuninatorType::String),
        ResultMetadata::new("branch", RuninatorType::String),
        ResultMetadata::new("dirty", RuninatorType::Boolean),
        ResultMetadata::new("action", RuninatorType::String),
    ]
}

fn archive_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("sha", RuninatorType::String),
        ResultMetadata::new("dirty", RuninatorType::Boolean),
        ResultMetadata::new("patch_path", RuninatorType::String),
        ResultMetadata::new("size_bytes", RuninatorType::Integer),
    ]
}

fn promotion_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("candidate_sha", RuninatorType::String),
        ResultMetadata::new("target_ref", RuninatorType::String),
        ResultMetadata::new("previous_target_sha", RuninatorType::String),
        ResultMetadata::new("pushed", RuninatorType::Boolean),
    ]
}
