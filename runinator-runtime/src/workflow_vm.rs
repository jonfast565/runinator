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
            // `Select` and `PureNode` are compiler-prototype opcodes. Keeping their records
            // decodable allows deployment-disabled scaffolding to be inspected, but executing
            // either would silently reintroduce reducer semantics. Phase 3/4 replaces them with
            // the dedicated opcodes below.
            // Phase 2 deliberately makes the eventual bytecode vocabulary visible before Phase 8
            // teaches the pure VM its semantics. Treating one of these as a no-op would corrupt a
            // persisted continuation; fail closed until its evaluator lands.
            WorkflowInstruction::Select { .. }
            | WorkflowInstruction::PureNode { .. }
            | WorkflowInstruction::Evaluate { .. }
            | WorkflowInstruction::BeginLoop { .. }
            | WorkflowInstruction::NextLoop { .. }
            | WorkflowInstruction::Reenter { .. }
            | WorkflowInstruction::BeginTry { .. }
            | WorkflowInstruction::EndTry { .. }
            | WorkflowInstruction::RegisterCompensation { .. }
            | WorkflowInstruction::BeginCompensation { .. }
            | WorkflowInstruction::Race { .. }
            | WorkflowInstruction::BeginMap { .. }
            | WorkflowInstruction::CheckInterrupt { .. }
            | WorkflowInstruction::ResumeInterrupt { .. }
            | WorkflowInstruction::DebugBoundary { .. }
            | WorkflowInstruction::SetOutput { .. } => {
                return fail(
                    continuation,
                    format!("unsupported workflow VM opcode: {instruction:?}"),
                );
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
    use runinator_models::{
        orchestration::GateKind,
        workflow_vm::{WorkflowEffectRequest, WorkflowInstruction},
        workflows::{
            WorkflowDefinition, WorkflowGraph, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef,
            WorkflowTransitions,
        },
    };
    use runinator_workflows::compile_workflow_module;
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
    fn rejects_a_finalized_opcode_until_its_phase_is_implemented() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::BeginLoop {
            loop_key: "items".into(),
            body: 0,
            exit: 0,
            max_iterations: None,
        }]);
        let WorkflowVmStep::Failed { message, .. } = step(&module, continuation()) else {
            panic!("expected an unsupported-opcode failure");
        };
        assert!(message.contains("unsupported workflow VM opcode"));
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
    fn parking_effects_yield_resume_and_restart_stably() {
        // Each parking request has exactly the same host-free lifecycle: stepping yields one
        // durable effect, replaying the unmodified continuation yields that same receipt, and a
        // settled typed value resumes into the next instruction. The coordination host never
        // needs a node-specific polling loop to make progress.
        let requests = vec![
            WorkflowEffectRequest::TimerDelay { seconds: 30 },
            WorkflowEffectRequest::Approval {
                prompt: Value::String("approve deployment".into()),
                expires_at: None,
            },
            WorkflowEffectRequest::Gate {
                kind: GateKind::Manual,
                condition: Default::default(),
                poll_interval_seconds: 30,
                deadline_seconds: Some(300),
                continue_on_timeout: false,
                label: Some("production".into()),
                metadata: Value::Null,
            },
            WorkflowEffectRequest::Signal {
                key: "release-ready".into(),
                filter: Some(Value::String("release-42".into())),
            },
            WorkflowEffectRequest::Input {
                prompt: Some("version".into()),
                schema: Value::Null,
            },
            WorkflowEffectRequest::EventWait {
                event_type: "build.finished".into(),
                filter: None,
                max_events: Some(1),
            },
            WorkflowEffectRequest::ChildRun {
                workflow_id: Some(Uuid::nil()),
                workflow_name: None,
                input: Value::Null,
                wait: true,
                reuse_open_run: false,
                run_name: None,
            },
            WorkflowEffectRequest::AwaitRun {
                workflow: "child".into(),
                key: None,
                run_id: None,
                mode: "all".into(),
            },
        ];

        for request in requests {
            let module = WorkflowModule::new(vec![
                WorkflowInstruction::Effect {
                    request: request.clone(),
                },
                WorkflowInstruction::Return,
            ]);
            let start = continuation();
            let WorkflowVmStep::Yield {
                continuation: waiting,
                effect_id,
                sequence,
                request: yielded,
            } = step(&module, start.clone())
            else {
                panic!("parking request must yield");
            };
            assert_eq!(yielded, request);

            let restarted: WorkflowContinuation =
                serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
            let WorkflowVmStep::Yield {
                effect_id: replayed_id,
                sequence: replayed_sequence,
                request: replayed_request,
                ..
            } = step(&module, restarted)
            else {
                panic!("restarted parking request must yield");
            };
            assert_eq!(
                (replayed_id, replayed_sequence, replayed_request),
                (effect_id, sequence, request)
            );

            let value = Value::String("settled".into());
            let WorkflowVmStep::Complete {
                value: completed, ..
            } = resume(&module, waiting, Ok(value.clone()))
            else {
                panic!("settled parking request must resume");
            };
            assert_eq!(completed, value);
        }
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

    #[test]
    fn compiled_linear_graph_reaches_the_same_terminal_result() {
        let module = compile_workflow_module(&WorkflowDefinition {
            id: None,
            name: "linear".into(),
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    vm_node("start", WorkflowNodeKind::Start, Some("end")),
                    vm_node("end", WorkflowNodeKind::End, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        })
        .unwrap();

        let WorkflowVmStep::Complete {
            value,
            continuation,
        } = step(&module, continuation())
        else {
            panic!("expected the compiled linear graph to complete");
        };
        assert_eq!(value, Value::Null);
        assert_eq!(continuation.status, WorkflowContinuationStatus::Succeeded);
    }

    fn vm_node(id: &str, kind: WorkflowNodeKind, next: Option<&str>) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            kind,
            skipped: false,
            locked: false,
            action: None,
            parameters: Default::default(),
            wait: Default::default(),
            condition: Default::default(),
            transitions: WorkflowTransitions {
                next: next.map(WorkflowNodeRef::new),
                ..Default::default()
            },
            retry: Default::default(),
            timeout_seconds: None,
            max_iterations: None,
            subflow_id: None,
            subflow: Default::default(),
            reentry: Default::default(),
            compensation: None,
        }
    }
}
