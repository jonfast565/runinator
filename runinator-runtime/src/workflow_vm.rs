//! Host-free interpreter for [`runinator_models::workflow_vm::WorkflowModule`].
//!
//! The machine stops at durable boundaries. Its caller is responsible for assigning effect ids and
//! atomically persisting the returned continuation and effect receipt.

use runinator_models::{
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest,
        WorkflowInstruction, WorkflowModule,
    },
};

const MAX_INLINE_INSTRUCTIONS: usize = 1_024;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowVmStep {
    Yield {
        continuation: WorkflowContinuation,
        effect_id: uuid::Uuid,
        sequence: u64,
        request: WorkflowEffectRequest,
    },
    Fork {
        parent: WorkflowContinuation,
        children: Vec<WorkflowContinuation>,
        join_key: String,
    },
    Joined {
        continuation: WorkflowContinuation,
        join_key: String,
        value: Value,
    },
    Complete {
        continuation: WorkflowContinuation,
        value: Value,
    },
    Failed {
        continuation: WorkflowContinuation,
        message: String,
    },
}

/// Resume a continuation after the host durably settled its sole outstanding effect.
pub fn resume(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    result: Result<Value, String>,
) -> WorkflowVmStep {
    if continuation.status != WorkflowContinuationStatus::Waiting
        || continuation.awaiting_effect_id.is_none()
    {
        return fail(
            continuation,
            "attempted to resume a continuation that is not waiting for an effect".into(),
        );
    }
    continuation.awaiting_effect_id = None;
    continuation.status = WorkflowContinuationStatus::Runnable;
    match result {
        Ok(value) => continuation.stack.push(value),
        Err(message) => {
            continuation.status = WorkflowContinuationStatus::Failed;
            return WorkflowVmStep::Failed {
                continuation,
                message,
            };
        }
    }
    step(module, continuation)
}

/// Run a continuation until it reaches its next durable boundary.
pub fn step(module: &WorkflowModule, mut continuation: WorkflowContinuation) -> WorkflowVmStep {
    if let Err(error) = module.ensure_supported() {
        return fail(continuation, error.to_string());
    }
    if let Err(error) = continuation.ensure_supported() {
        return fail(continuation, error.to_string());
    }
    if continuation.module_version != module.version {
        let continuation_version = continuation.module_version;
        return fail(
            continuation,
            format!(
                "continuation module version {} does not match module version {}",
                continuation_version, module.version
            ),
        );
    }
    if continuation.status != WorkflowContinuationStatus::Runnable {
        return fail(
            continuation,
            "attempted to step a non-runnable continuation".into(),
        );
    }

    for _ in 0..MAX_INLINE_INSTRUCTIONS {
        let Some(instruction) = module.instructions.get(continuation.instruction_pointer) else {
            return fail(
                continuation,
                "instruction pointer is outside the workflow module".into(),
            );
        };
        match instruction {
            WorkflowInstruction::EnterNode { .. } => {
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::Const { value } => {
                continuation.stack.push(value.clone());
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::LoadLocal { name } => {
                continuation.stack.push(
                    continuation
                        .locals
                        .get(name)
                        .cloned()
                        .unwrap_or(Value::Null),
                );
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::StoreLocal { name } => {
                let Some(value) = continuation.stack.pop() else {
                    return fail(
                        continuation,
                        format!("store_local '{name}' needs a stack value"),
                    );
                };
                continuation.locals.insert(name.clone(), value);
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::Pop => {
                if continuation.stack.pop().is_none() {
                    return fail(continuation, "pop needs a stack value".into());
                }
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::Jump { target } => continuation.instruction_pointer = *target,
            WorkflowInstruction::JumpIfFalse { target } => {
                let Some(value) = continuation.stack.pop() else {
                    return fail(continuation, "jump_if_false needs a stack value".into());
                };
                continuation.instruction_pointer = if truthy(&value) {
                    continuation.instruction_pointer + 1
                } else {
                    *target
                };
            }
            WorkflowInstruction::Branch { branches, default } => {
                // Conditions are evaluated against the continuation's frozen local context. A
                // richer expression failure is a deterministic VM failure, never a host callback.
                let context = Value::Object(
                    continuation
                        .locals
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                );
                let target = branches.iter().find_map(|branch| {
                    runinator_compute::evaluate_workflow_condition(&branch.condition, &context)
                        .ok()
                        .filter(|matched| *matched)
                        .map(|_| branch.target)
                });
                match target.or(*default) {
                    Some(target) => continuation.instruction_pointer = target,
                    None => return fail(continuation, "branch has no matching target".into()),
                }
            }
            WorkflowInstruction::Select {
                kind,
                configuration,
                targets,
                default,
            } => {
                // The complete per-kind evaluators are introduced with the corresponding VM frame
                // semantics. Until then, validated single-target selectors are deterministic and
                // multi-target selectors fail closed instead of guessing a graph transition.
                let target = match (targets.as_slice(), default) {
                    ([target], _) => Some(*target),
                    ([], Some(target)) => Some(*target),
                    _ => None,
                };
                let Some(target) = target else {
                    return fail(
                        continuation,
                        format!(
                            "selector {kind:?} requires its dedicated VM evaluator: {configuration}"
                        ),
                    );
                };
                continuation.instruction_pointer = target;
            }
            WorkflowInstruction::PureNode { .. } => {
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::Effect { request } => {
                let sequence = continuation.next_effect_sequence;
                continuation.next_effect_sequence += 1;
                let effect_id = stable_id(continuation.id, &format!("effect:{sequence}"));
                continuation.instruction_pointer += 1;
                continuation.awaiting_effect_id = Some(effect_id);
                continuation.status = WorkflowContinuationStatus::Waiting;
                return WorkflowVmStep::Yield {
                    continuation,
                    effect_id,
                    sequence,
                    request: request.clone(),
                };
            }
            WorkflowInstruction::Fork { targets, join_key } => {
                if targets.is_empty() {
                    return fail(continuation, "fork needs at least one target".into());
                }
                let mut children = Vec::with_capacity(targets.len());
                for (branch, target) in targets.iter().enumerate() {
                    let mut child = continuation.clone();
                    child.id = stable_id(continuation.id, &format!("fork:{join_key}:{branch}"));
                    child.parent_id = Some(continuation.id);
                    child.fork_key = Some(join_key.clone());
                    child.instruction_pointer = *target;
                    child.awaiting_effect_id = None;
                    child.status = WorkflowContinuationStatus::Runnable;
                    children.push(child);
                }
                continuation.instruction_pointer += 1;
                continuation.status = WorkflowContinuationStatus::Joined;
                return WorkflowVmStep::Fork {
                    parent: continuation,
                    children,
                    join_key: join_key.clone(),
                };
            }
            WorkflowInstruction::Join { join_key } => {
                let value = continuation.stack.pop().unwrap_or(Value::Null);
                continuation.status = WorkflowContinuationStatus::Joined;
                return WorkflowVmStep::Joined {
                    continuation,
                    join_key: join_key.clone(),
                    value,
                };
            }
            WorkflowInstruction::Return => {
                let value = continuation.stack.pop().unwrap_or(Value::Null);
                continuation.status = WorkflowContinuationStatus::Succeeded;
                return WorkflowVmStep::Complete {
                    continuation,
                    value,
                };
            }
            WorkflowInstruction::Fail { message } => return fail(continuation, message.clone()),
        }
    }
    fail(continuation, "workflow instruction budget exhausted".into())
}

fn fail(mut continuation: WorkflowContinuation, message: String) -> WorkflowVmStep {
    continuation.status = WorkflowContinuationStatus::Failed;
    WorkflowVmStep::Failed {
        continuation,
        message,
    }
}

fn stable_id(namespace: uuid::Uuid, name: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&namespace, name.as_bytes())
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::workflow_vm::{WorkflowEffectRequest, WorkflowInstruction};
    use uuid::Uuid;

    fn continuation() -> WorkflowContinuation {
        WorkflowContinuation::start(Uuid::now_v7(), 1)
    }

    #[test]
    fn yields_once_and_resumes_with_the_effect_value() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Effect {
                request: WorkflowEffectRequest::Timer { due_at: 1 },
            },
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Yield {
            continuation,
            effect_id,
            sequence,
            ..
        } = step(&module, continuation())
        else {
            panic!("expected effect yield");
        };
        assert_eq!(sequence, 0);
        assert_eq!(continuation.awaiting_effect_id, Some(effect_id));
        assert_eq!(continuation.status, WorkflowContinuationStatus::Waiting);
        let WorkflowVmStep::Complete { value, .. } =
            resume(&module, continuation, Ok(Value::String("done".into())))
        else {
            panic!("expected completion");
        };
        assert_eq!(value, Value::String("done".into()));
    }

    #[test]
    fn fork_makes_independently_addressable_children() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Fork {
                targets: vec![1, 2],
                join_key: "all".into(),
            },
            WorkflowInstruction::Return,
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Fork {
            parent, children, ..
        } = step(&module, continuation())
        else {
            panic!("expected fork");
        };
        assert_eq!(parent.status, WorkflowContinuationStatus::Joined);
        assert_eq!(children.len(), 2);
        assert_ne!(children[0].id, children[1].id);
        assert!(
            children
                .iter()
                .all(|child| child.parent_id == Some(parent.id))
        );
    }

    #[test]
    fn refuses_a_duplicate_resume_after_the_wait_was_consumed() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Effect {
                request: WorkflowEffectRequest::Timer { due_at: 1 },
            },
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
            panic!("expected effect yield");
        };
        let WorkflowVmStep::Complete { continuation, .. } =
            resume(&module, continuation, Ok(Value::Null))
        else {
            panic!("expected completion");
        };
        assert!(matches!(
            resume(&module, continuation, Ok(Value::Null)),
            WorkflowVmStep::Failed { .. }
        ));
    }

    #[test]
    fn duplicate_drive_yields_the_same_logical_effect() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Timer { due_at: 1 },
        }]);
        let continuation = continuation();
        let WorkflowVmStep::Yield {
            effect_id: first_id,
            sequence: first_sequence,
            ..
        } = step(&module, continuation.clone())
        else {
            panic!("expected yield");
        };
        let WorkflowVmStep::Yield {
            effect_id: second_id,
            sequence: second_sequence,
            ..
        } = step(&module, continuation)
        else {
            panic!("expected yield");
        };
        assert_eq!((first_id, first_sequence), (second_id, second_sequence));
    }

    #[test]
    fn duplicate_fork_drive_preserves_child_identity_and_order() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::Fork {
            targets: vec![1, 2],
            join_key: "all".into(),
        }]);
        let continuation = continuation();
        let WorkflowVmStep::Fork {
            children: first, ..
        } = step(&module, continuation.clone())
        else {
            panic!("expected fork");
        };
        let WorkflowVmStep::Fork {
            children: second, ..
        } = step(&module, continuation)
        else {
            panic!("expected fork");
        };
        assert_eq!(
            first.iter().map(|child| child.id).collect::<Vec<_>>(),
            second.iter().map(|child| child.id).collect::<Vec<_>>()
        );
    }
}
