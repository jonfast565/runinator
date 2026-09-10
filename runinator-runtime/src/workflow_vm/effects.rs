//! effects behavior for the workflow interpreter.

use super::context::{local_context, stable_id};
use super::failure::fail;
use super::foreign_code::enrich_foreign_code_input;
use super::{InstructionOutcome, WorkflowVmStep};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest,
};
use std::ops::ControlFlow::Break;

pub(super) fn yield_effect(
    mut continuation: WorkflowContinuation,
    request: WorkflowEffectRequest,
) -> WorkflowVmStep {
    let request = match resolve_effect_request(request, &continuation) {
        Ok(request) => request,
        Err(message) => return fail(continuation, message),
    };
    let sequence = continuation.next_effect_sequence;
    continuation.next_effect_sequence += 1;
    let effect_id = stable_id(continuation.id, &format!("effect:{sequence}"));
    continuation.instruction_pointer += 1;
    continuation.awaiting_effect_id = Some(effect_id);
    continuation.status = WorkflowContinuationStatus::Waiting;
    WorkflowVmStep::Yield {
        continuation,
        effect_id,
        sequence,
        request: Box::new(request),
    }
}

/// Freeze every context-dependent value before the request crosses the durable effect boundary.
/// A resumed delivery must never re-read mutable workflow inputs or carry authoring `$ref` objects
/// to a provider/infrastructure host.
pub(super) fn resolve_effect_request(
    request: WorkflowEffectRequest,
    continuation: &WorkflowContinuation,
) -> Result<WorkflowEffectRequest, String> {
    let context = local_context(continuation);
    let resolve = |value: Value| {
        runinator_workflows::resolve_value_refs(&value, &context).map_err(|error| error.to_string())
    };
    Ok(match request {
        WorkflowEffectRequest::Action {
            provider,
            function,
            input,
            timeout_seconds,
            retry,
            tags,
            required_labels,
            workspace_affinity,
            execution_profile,
            idempotency_key,
            function_binding,
        } => {
            let input = resolve(input)?;
            let input = if provider == "std" && function == "code" {
                enrich_foreign_code_input(input, &context)?
            } else {
                input
            };
            WorkflowEffectRequest::Action {
                provider,
                function,
                input,
                timeout_seconds,
                retry,
                tags,
                required_labels,
                workspace_affinity: workspace_affinity
                    .map(|value| {
                        if let Some(default) = value.get("$workspace_default") {
                            let resolved = resolve(default.clone())?;
                            let mut attachment: runinator_models::workspaces::WorkspaceAttachment =
                                resolved.decode().map_err(|error| error.to_string())?;
                            attachment.follow_run = true;
                            Value::encode(&attachment).map_err(|error| error.to_string())
                        } else {
                            resolve(value)
                        }
                    })
                    .transpose()?,
                execution_profile,
                idempotency_key: idempotency_key.map(resolve).transpose()?,
                function_binding,
            }
        }
        WorkflowEffectRequest::Approval { prompt, expires_at } => WorkflowEffectRequest::Approval {
            prompt: resolve(prompt)?,
            expires_at,
        },
        WorkflowEffectRequest::Gate {
            kind,
            condition,
            poll_interval_seconds,
            deadline_seconds,
            continue_on_timeout,
            label,
            metadata,
        } => WorkflowEffectRequest::Gate {
            kind,
            condition: runinator_models::workflows::WorkflowCondition::from_value(resolve(
                condition.to_value(),
            )?),
            poll_interval_seconds,
            deadline_seconds,
            continue_on_timeout,
            label,
            metadata: resolve(metadata)?,
        },
        WorkflowEffectRequest::Signal { key, filter } => WorkflowEffectRequest::Signal {
            key,
            filter: filter.map(resolve).transpose()?,
        },
        WorkflowEffectRequest::Input { prompt, schema } => WorkflowEffectRequest::Input {
            prompt,
            schema: resolve(schema)?,
        },
        WorkflowEffectRequest::EventWait {
            event_type,
            filter,
            max_events,
        } => WorkflowEffectRequest::EventWait {
            event_type,
            filter: filter.map(resolve).transpose()?,
            max_events,
        },
        WorkflowEffectRequest::ChildRun {
            workflow_id,
            workflow_name,
            workflow_revision,
            workflow_revision_digest,
            input,
            wait,
            reuse_open_run,
            run_name,
        } => WorkflowEffectRequest::ChildRun {
            workflow_id,
            workflow_name,
            workflow_revision,
            workflow_revision_digest,
            input: resolve(input)?,
            wait,
            reuse_open_run,
            run_name: run_name.map(resolve).transpose()?,
        },
        WorkflowEffectRequest::AwaitRun {
            workflow,
            key,
            run_id,
            mode,
        } => WorkflowEffectRequest::AwaitRun {
            workflow,
            key: key.map(resolve).transpose()?,
            run_id: run_id.map(resolve).transpose()?,
            mode,
        },
        WorkflowEffectRequest::Coordination { kind, input } => {
            WorkflowEffectRequest::Coordination {
                kind,
                input: resolve(input)?,
            }
        }
        request @ (WorkflowEffectRequest::Timer { .. }
        | WorkflowEffectRequest::TimerDelay { .. }
        | WorkflowEffectRequest::MutexAcquire { .. }) => request,
    })
}

pub(super) fn effect(
    continuation: WorkflowContinuation,
    request: &WorkflowEffectRequest,
) -> InstructionOutcome {
    Break(yield_effect(continuation, request.clone()))
}
