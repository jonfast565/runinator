//! Validated workflow graph to versioned workflow bytecode lowering.

use std::collections::HashMap;

use runinator_compute::WorkflowValidationError;
use runinator_models::{
    value::Value,
    workflow_vm::{
        WorkflowEffectRequest, WorkflowInstruction, WorkflowModule, WorkflowSourceMapEntry,
        WorkflowVmBranch,
    },
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef},
};

use crate::{parse_wait_parameters, target_slots, validate_workflow};

enum Pending {
    Instruction(WorkflowInstruction),
    Jump(String),
    Branch(
        Vec<(runinator_models::workflows::WorkflowCondition, String)>,
        Option<String>,
    ),
    Select(WorkflowNodeKind, Value, Vec<String>, Option<String>),
    Fork(Vec<String>, String),
}

/// Compile a validated authoring graph into an immutable module with a mandatory graph source map.
pub fn compile_workflow_module(
    workflow: &WorkflowDefinition,
) -> Result<WorkflowModule, WorkflowValidationError> {
    let (start, nodes) = validate_workflow(workflow)?;
    let mut ordered = Vec::with_capacity(nodes.len());
    let start_node = nodes
        .iter()
        .find(|node| node.id == start)
        .expect("validation proved the start node exists");
    ordered.push(start_node);
    ordered.extend(nodes.iter().filter(|node| node.id != start));

    let mut pending = Vec::new();
    let mut starts = HashMap::new();
    let mut ranges = Vec::new();
    for node in ordered {
        let begin = pending.len();
        starts.insert(node.id.clone(), begin);
        pending.push(Pending::Instruction(WorkflowInstruction::EnterNode {
            node_id: node.id.clone(),
        }));
        lower_node(node, &mut pending)?;
        ranges.push((begin, pending.len(), node.id.clone()));
    }

    let resolve = |node: &str| {
        starts
            .get(node)
            .copied()
            .ok_or_else(|| WorkflowValidationError::MissingTransition {
                node: "compiler".into(),
                target: node.into(),
            })
    };
    let mut instructions = Vec::with_capacity(pending.len());
    for instruction in pending {
        instructions.push(match instruction {
            Pending::Instruction(instruction) => instruction,
            Pending::Jump(target) => WorkflowInstruction::Jump {
                target: resolve(&target)?,
            },
            Pending::Branch(branches, default) => WorkflowInstruction::Branch {
                branches: branches
                    .into_iter()
                    .map(|(condition, target)| {
                        Ok(WorkflowVmBranch {
                            condition,
                            target: resolve(&target)?,
                        })
                    })
                    .collect::<Result<Vec<_>, WorkflowValidationError>>()?,
                default: default.as_deref().map(resolve).transpose()?,
            },
            Pending::Select(kind, configuration, targets, default) => WorkflowInstruction::Select {
                kind,
                configuration,
                targets: targets
                    .iter()
                    .map(|target| resolve(target))
                    .collect::<Result<Vec<_>, _>>()?,
                default: default.as_deref().map(resolve).transpose()?,
            },
            Pending::Fork(targets, join_key) => WorkflowInstruction::Fork {
                targets: targets
                    .iter()
                    .map(|target| resolve(target))
                    .collect::<Result<Vec<_>, _>>()?,
                join_key,
            },
        });
    }
    Ok(WorkflowModule {
        version: runinator_models::workflow_vm::WORKFLOW_VM_VERSION,
        instructions,
        source_map: ranges
            .into_iter()
            .map(
                |(instruction_start, instruction_end, node_id)| WorkflowSourceMapEntry {
                    instruction_start,
                    instruction_end,
                    node_id,
                    edge_label: None,
                    source_span: None,
                },
            )
            .collect(),
    })
}

fn lower_node(
    node: &WorkflowNode,
    output: &mut Vec<Pending>,
) -> Result<(), WorkflowValidationError> {
    let configuration = || {
        serde_json::to_value(node)
            .map(Value::from)
            .unwrap_or(Value::Null)
    };
    let next = || {
        node.transitions
            .on_success
            .as_ref()
            .or(node.transitions.next.as_ref())
            .map(WorkflowNodeRef::as_str)
            .map(str::to_owned)
    };
    let jump_next = |output: &mut Vec<Pending>| {
        if let Some(target) = next() {
            output.push(Pending::Jump(target));
        }
    };

    match node.kind {
        WorkflowNodeKind::Start | WorkflowNodeKind::Interrupt => jump_next(output),
        WorkflowNodeKind::End => output.push(Pending::Instruction(WorkflowInstruction::Return)),
        WorkflowNodeKind::Fail => output.push(Pending::Instruction(WorkflowInstruction::Fail {
            message: node
                .parameters
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("workflow failed")
                .to_string(),
        })),
        WorkflowNodeKind::Resume => {
            output.push(Pending::Instruction(WorkflowInstruction::PureNode {
                kind: node.kind.clone(),
                configuration: configuration(),
            }));
            jump_next(output);
        }
        WorkflowNodeKind::Action => {
            let action = node
                .action
                .as_ref()
                .ok_or_else(|| WorkflowValidationError::MissingAction(node.id.clone()))?;
            output.push(Pending::Instruction(WorkflowInstruction::Effect {
                request: WorkflowEffectRequest::Action {
                    provider: action.provider.clone(),
                    function: action.function.clone(),
                    input: serde_json::to_value(&action.configuration)
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                    timeout_seconds: Some(action.timeout_seconds),
                },
            }));
            jump_next(output);
        }
        WorkflowNodeKind::Wait => {
            output.push(Pending::Instruction(WorkflowInstruction::Effect {
                request: WorkflowEffectRequest::TimerDelay {
                    seconds: parse_wait_parameters(node).seconds,
                },
            }));
            jump_next(output);
        }
        WorkflowNodeKind::Approval => durable(node, "approval", configuration(), output, next()),
        WorkflowNodeKind::Gate => durable(node, "gate", configuration(), output, next()),
        WorkflowNodeKind::Signal => durable(node, "signal", configuration(), output, next()),
        WorkflowNodeKind::Input => durable(node, "input", configuration(), output, next()),
        WorkflowNodeKind::Subflow => durable(node, "child_run", configuration(), output, next()),
        WorkflowNodeKind::Invocation => {
            durable(node, "invocation", configuration(), output, next())
        }
        WorkflowNodeKind::Audit => durable(node, "audit", configuration(), output, next()),
        WorkflowNodeKind::Checkpoint => {
            durable(node, "checkpoint", configuration(), output, next())
        }
        WorkflowNodeKind::Mutex => durable(node, "mutex", configuration(), output, next()),
        WorkflowNodeKind::Throttle => durable(node, "throttle", configuration(), output, next()),
        WorkflowNodeKind::Cooldown => durable(node, "cooldown", configuration(), output, next()),
        WorkflowNodeKind::AwaitRun => durable(node, "await_run", configuration(), output, next()),
        WorkflowNodeKind::Debounce => durable(node, "debounce", configuration(), output, next()),
        WorkflowNodeKind::Collect => durable(node, "collect", configuration(), output, next()),
        WorkflowNodeKind::Barrier => durable(node, "barrier", configuration(), output, next()),
        WorkflowNodeKind::CircuitBreaker => {
            durable(node, "circuit_breaker", configuration(), output, next())
        }
        WorkflowNodeKind::EventSource => {
            durable(node, "event_source", configuration(), output, next())
        }
        WorkflowNodeKind::Output => durable(node, "output", configuration(), output, next()),
        WorkflowNodeKind::Config | WorkflowNodeKind::Assert | WorkflowNodeKind::Transform => {
            output.push(Pending::Instruction(WorkflowInstruction::PureNode {
                kind: node.kind.clone(),
                configuration: configuration(),
            }));
            jump_next(output);
        }
        WorkflowNodeKind::Condition => {
            let branches = node
                .transitions
                .branches
                .iter()
                .map(|branch| (branch.when.clone(), branch.target.as_str().to_string()))
                .collect();
            output.push(Pending::Branch(branches, next()));
        }
        WorkflowNodeKind::Switch
        | WorkflowNodeKind::Toggle
        | WorkflowNodeKind::Percentage
        | WorkflowNodeKind::Loop
        | WorkflowNodeKind::Try
        | WorkflowNodeKind::Map => {
            let targets = target_slots(node)?
                .into_iter()
                .map(|slot| slot.target.as_str().to_string())
                .collect();
            output.push(Pending::Select(
                node.kind.clone(),
                configuration(),
                targets,
                next(),
            ));
        }
        WorkflowNodeKind::Parallel | WorkflowNodeKind::Race => {
            let targets = target_slots(node)?
                .into_iter()
                .map(|slot| slot.target.as_str().to_string())
                .collect();
            output.push(Pending::Fork(targets, node.id.clone()));
        }
        WorkflowNodeKind::Join => output.push(Pending::Instruction(WorkflowInstruction::Join {
            join_key: node.id.clone(),
        })),
    }
    Ok(())
}

fn durable(
    node: &WorkflowNode,
    kind: &str,
    input: Value,
    output: &mut Vec<Pending>,
    next: Option<String>,
) {
    output.push(Pending::Instruction(WorkflowInstruction::Effect {
        request: WorkflowEffectRequest::Coordination {
            kind: kind.to_string(),
            input,
        },
    }));
    if let Some(next) = next {
        output.push(Pending::Jump(next));
    } else if !matches!(node.kind, WorkflowNodeKind::End | WorkflowNodeKind::Fail) {
        output.push(Pending::Instruction(WorkflowInstruction::Return));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::workflows::{WorkflowGraph, WorkflowTransitions};

    #[test]
    fn compiles_graph_blocks_with_complete_source_map() {
        let definition = WorkflowDefinition {
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
                    node("start", WorkflowNodeKind::Start, Some("end")),
                    node("end", WorkflowNodeKind::End, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };
        let module = compile_workflow_module(&definition).unwrap();
        assert_eq!(module.source_map.len(), 2);
        assert!(
            (0..module.instructions.len()).all(|ip| module.graph_location(ip).is_some()),
            "every instruction must map back to an authoring node"
        );
        assert!(matches!(
            module.instructions.last(),
            Some(WorkflowInstruction::Return)
        ));
    }

    fn node(id: &str, kind: WorkflowNodeKind, next: Option<&str>) -> WorkflowNode {
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
