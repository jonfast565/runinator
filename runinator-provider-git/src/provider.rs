use runinator_provider_support::process_runner::{NativeProcessRunner, ProcessRunner};
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
use crate::errors::{
    INVALID_PARAMS, IO_ERROR, REVISION_MISMATCH, UNSUPPORTED_ACTION, WORKSPACE_SAFETY,
};
use crate::params::{
    ArchivePatchParams, AttemptWorktreeParams, CleanupParams, CommitParams, PrepareCheckoutParams,
    PromoteRevisionParams, PushParams, WorkspaceParams, WorktreeParams, parse_params,
};

#[allow(non_upper_case_globals)]
pub const GitProvider: GitProvider = GitProvider {
    runner: NativeProcessRunner,
};

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
    runner: &dyn ProcessRunner,
    action: &str,
    workspace: &str,
    timeout: i64,
    token: &runinator_plugin::cancel::CancellationToken,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Result<TaskExecutionResult, SendableError> {
    let sha = run_command(
        runner,
        "git",
        &["-C", workspace, "rev-parse", "HEAD"],
        timeout,
        token,
        sink,
    )?;
    let branch = run_command(
        runner,
        "git",
        &["-C", workspace, "branch", "--show-current"],
        timeout,
        token,
        sink,
    )?;
    let status = run_command(
        runner,
        "git",
        &["-C", workspace, "status", "--porcelain"],
        timeout,
        token,
        sink,
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
    runner: &dyn ProcessRunner,
    params: &PromoteRevisionParams,
    workspace: &str,
    timeout: i64,
    token: &runinator_plugin::cancel::CancellationToken,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Result<TaskExecutionResult, SendableError> {
    if !params.target_ref.starts_with("refs/heads/") {
        return Err(WORKSPACE_SAFETY.error("target_ref must be beneath refs/heads/"));
    }
    let repo = params.repo.as_deref().unwrap_or(workspace);
    let candidate_expression = format!("{}^{{commit}}", params.candidate_sha);
    let candidate = run_command(
        runner,
        "git",
        &["-C", repo, "rev-parse", "--verify", &candidate_expression],
        timeout,
        token,
        sink,
    )?;
    let candidate = candidate.trim();
    let workspace_head = run_command(
        runner,
        "git",
        &["-C", workspace, "rev-parse", "HEAD"],
        timeout,
        token,
        sink,
    )?;
    if workspace_head.trim() != candidate {
        return Err(REVISION_MISMATCH.error(format!(
            "workspace HEAD {} does not match candidate {candidate}",
            workspace_head.trim()
        )));
    }

    let current = run_command_output(
        runner,
        "git",
        &["-C", repo, "rev-parse", "--verify", &params.target_ref],
        timeout,
        token,
        sink,
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
        run_command(runner, "git", &args, timeout, token, sink)?;
    }

    let mut pushed = false;
    if params.push.unwrap_or(false) {
        let remote = params.remote.as_deref().unwrap_or("origin");
        let remote_ref = run_command(
            runner,
            "git",
            &["-C", repo, "ls-remote", remote, &params.target_ref],
            timeout,
            token,
            sink,
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
                    runner,
                    "git",
                    &["-C", repo, "push", &lease, remote, &refspec],
                    timeout,
                    token,
                    sink,
                )?;
            } else {
                run_command(
                    runner,
                    "git",
                    &["-C", repo, "push", remote, &refspec],
                    timeout,
                    token,
                    sink,
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

mod git_result;
use git_result::GitResult;

mod prepared_checkout_result;
use prepared_checkout_result::PreparedCheckoutResult;

mod git_provider;
pub use git_provider::GitProvider;
