//! selectors behavior for the workflow interpreter.

use super::InstructionOutcome;
use super::context::{local_context, truthy};
use super::failure::{fail, handle_failure};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{WorkflowContinuation, WorkflowModule};
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn select_target(
    kind: &runinator_models::workflows::WorkflowNodeKind,
    configuration: &Value,
    targets: &[usize],
    default: Option<usize>,
    continuation: &WorkflowContinuation,
) -> Result<Option<usize>, String> {
    let parameters = configuration
        .get("parameters")
        .ok_or_else(|| "selector configuration has no parameters".to_string())?;
    let context = local_context(continuation);
    let resolve = |value: &Value| {
        runinator_workflows::resolve_value_refs(value, &context).map_err(|error| error.to_string())
    };
    match kind {
        runinator_models::workflows::WorkflowNodeKind::Toggle => {
            let value = resolve(
                parameters
                    .get("value")
                    .ok_or_else(|| "toggle.value is required".to_string())?,
            )?;
            match targets {
                [on, off, ..] => Ok(Some(if truthy(&value) { *on } else { *off })),
                _ => Err("toggle needs on and off targets".into()),
            }
        }
        runinator_models::workflows::WorkflowNodeKind::Percentage => {
            let key = resolve(
                parameters
                    .get("key")
                    .ok_or_else(|| "percentage.key is required".to_string())?,
            )?;
            // Match the graph-layer percentage evaluator: a null key does not participate in a
            // rollout, so it follows the authored fallback rather than being hashed as JSON null.
            if key.is_null() {
                return Ok(default);
            }
            let buckets = parameters
                .get("buckets")
                .and_then(Value::as_array)
                .ok_or_else(|| "percentage.buckets must be an array".to_string())?;
            if buckets.len() != targets.len() {
                return Err("percentage targets do not match buckets".into());
            }
            let total = buckets
                .iter()
                .map(|bucket| bucket.get("weight").and_then(Value::as_i64).unwrap_or(0))
                .sum::<i64>();
            if total <= 0 {
                return Ok(default);
            }
            let encoded = serde_json::to_vec(&key).map_err(|error| error.to_string())?;
            let bucket = encoded.iter().fold(0_u64, |hash, byte| {
                hash.wrapping_mul(1099511628211).wrapping_add(*byte as u64)
            }) % total as u64;
            let mut edge = 0_u64;
            for (index, entry) in buckets.iter().enumerate() {
                edge += entry
                    .get("weight")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .max(0) as u64;
                if bucket < edge {
                    return Ok(Some(targets[index]));
                }
            }
            Ok(default)
        }
        _ => Err(format!("select does not support node kind {kind:?}")),
    }
}

pub(super) fn branch(
    mut continuation: WorkflowContinuation,
    branches: &[runinator_models::workflow_vm::WorkflowVmBranch],
    default: &Option<usize>,
) -> InstructionOutcome {
    // Conditions are evaluated against the continuation's frozen local context. A
    // richer expression failure is a deterministic VM failure, never a host callback.
    let context = local_context(&continuation);
    let target = branches.iter().find_map(|branch| {
        runinator_compute::evaluate_workflow_condition(&branch.condition, &context)
            .ok()
            .filter(|matched| *matched)
            .map(|_| branch.target)
    });
    match target.or(*default) {
        Some(target) => continuation.instruction_pointer = target,
        None => return Break(fail(continuation, "branch has no matching target".into())),
    }
    Continue(continuation)
}

pub(super) fn evaluate(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    invocation_module: &runinator_models::invocation::InvocationModule,
) -> InstructionOutcome {
    let context = local_context(&continuation);
    match runinator_compute::evaluate_module_pure(
        invocation_module,
        &context,
        &runinator_compute::CallableCatalog::builtin(),
    ) {
        Ok(value) => {
            continuation.stack.push(value);
            continuation.instruction_pointer += 1;
        }
        Err(error) => return Break(handle_failure(module, continuation, error.to_string())),
    }
    Continue(continuation)
}

pub(super) fn select(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    kind: &runinator_models::workflows::WorkflowNodeKind,
    configuration: &Value,
    targets: &[usize],
    default: &Option<usize>,
) -> InstructionOutcome {
    match select_target(kind, configuration, targets, *default, &continuation) {
        Ok(Some(target)) => continuation.instruction_pointer = target,
        Ok(None) => {
            return Break(handle_failure(
                module,
                continuation,
                "selector has no matching target".into(),
            ));
        }
        Err(message) => return Break(handle_failure(module, continuation, message)),
    };
    Continue(continuation)
}
