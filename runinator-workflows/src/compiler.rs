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

/// A symbolic basic-block address.  Keeping these until the final layout pass means graph
/// traversal never needs to guess an instruction offset while it is lowering a node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Label(String);

impl Label {
    fn node(node_id: &str) -> Self {
        Self(node_id.to_owned())
    }
}

/// An instruction before its graph targets have been laid out.
enum PendingInstruction {
    Instruction(WorkflowInstruction),
    Jump(Label),
    Branch(
        Vec<(runinator_models::workflows::WorkflowCondition, Label)>,
        Option<Label>,
    ),
    Select(WorkflowNodeKind, Value, Vec<Label>, Option<Label>),
    Fork(Vec<Label>, String),
}

/// A node-owned basic block.  The block boundary is also the unit recorded in the module's
/// source map, so every instruction generated for a graph node has one stable graph location.
struct BasicBlock {
    label: Label,
    node_id: String,
    instructions: Vec<PendingInstruction>,
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

    let mut blocks = Vec::with_capacity(ordered.len());
    for node in ordered {
        let mut instructions = vec![PendingInstruction::Instruction(
            WorkflowInstruction::EnterNode {
                node_id: node.id.clone(),
            },
        )];
        lower_node(node, &mut instructions)?;
        blocks.push(BasicBlock {
            label: Label::node(&node.id),
            node_id: node.id.clone(),
            instructions,
        });
    }

    // Lay out blocks before resolving target labels.  This two-pass shape makes forward edges,
    // backward edges, and future synthetic blocks use exactly the same fixup path.
    let mut starts = HashMap::new();
    let mut ranges = Vec::new();
    let mut instruction_count = 0;
    for block in &blocks {
        let begin = instruction_count;
        instruction_count += block.instructions.len();
        starts.insert(block.label.clone(), begin);
        ranges.push((begin, instruction_count, block.node_id.clone()));
    }

    let resolve = |label: &Label| {
        starts
            .get(label)
            .copied()
            .ok_or_else(|| WorkflowValidationError::MissingTransition {
                node: "compiler".into(),
                target: label.0.clone(),
            })
    };
    let mut instructions = Vec::with_capacity(instruction_count);
    for instruction in blocks.into_iter().flat_map(|block| block.instructions) {
        instructions.push(match instruction {
            PendingInstruction::Instruction(instruction) => instruction,
            PendingInstruction::Jump(target) => WorkflowInstruction::Jump {
                target: resolve(&target)?,
            },
            PendingInstruction::Branch(branches, default) => WorkflowInstruction::Branch {
                branches: branches
                    .into_iter()
                    .map(|(condition, target)| {
                        Ok(WorkflowVmBranch {
                            condition,
                            target: resolve(&target)?,
                        })
                    })
                    .collect::<Result<Vec<_>, WorkflowValidationError>>()?,
                default: default.as_ref().map(resolve).transpose()?,
            },
            PendingInstruction::Select(kind, configuration, targets, default) => {
                WorkflowInstruction::Select {
                    kind,
                    configuration,
                    targets: targets
                        .iter()
                        .map(|target| resolve(target))
                        .collect::<Result<Vec<_>, _>>()?,
                    default: default.as_ref().map(resolve).transpose()?,
                }
            }
            PendingInstruction::Fork(targets, join_key) => WorkflowInstruction::Fork {
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
                    version: runinator_models::workflow_vm::WORKFLOW_SOURCE_MAP_VERSION,
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
    output: &mut Vec<PendingInstruction>,
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
    let jump_next = |output: &mut Vec<PendingInstruction>| {
        if let Some(target) = next() {
            output.push(PendingInstruction::Jump(Label::node(&target)));
        }
    };

    match node.kind {
        WorkflowNodeKind::Start | WorkflowNodeKind::Interrupt => jump_next(output),
        WorkflowNodeKind::End => {
            output.push(PendingInstruction::Instruction(WorkflowInstruction::Return))
        }
        WorkflowNodeKind::Fail => {
            output.push(PendingInstruction::Instruction(WorkflowInstruction::Fail {
                message: node
                    .parameters
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("workflow failed")
                    .to_string(),
            }))
        }
        WorkflowNodeKind::Resume => {
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::PureNode {
                    kind: node.kind.clone(),
                    configuration: configuration(),
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Action => {
            let action = node
                .action
                .as_ref()
                .ok_or_else(|| WorkflowValidationError::MissingAction(node.id.clone()))?;
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::Action {
                        provider: action.provider.clone(),
                        function: action.function.clone(),
                        input: serde_json::to_value(&action.configuration)
                            .map(Value::from)
                            .unwrap_or(Value::Null),
                        timeout_seconds: Some(action.timeout_seconds),
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Wait => {
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::TimerDelay {
                        seconds: parse_wait_parameters(node).seconds,
                    },
                },
            ));
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
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::PureNode {
                    kind: node.kind.clone(),
                    configuration: configuration(),
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Condition => {
            let branches = node
                .transitions
                .branches
                .iter()
                .map(|branch| (branch.when.clone(), Label::node(branch.target.as_str())))
                .collect();
            output.push(PendingInstruction::Branch(
                branches,
                next().map(|target| Label::node(&target)),
            ));
        }
        WorkflowNodeKind::Switch
        | WorkflowNodeKind::Toggle
        | WorkflowNodeKind::Percentage
        | WorkflowNodeKind::Loop
        | WorkflowNodeKind::Try
        | WorkflowNodeKind::Map => {
            let targets = target_slots(node)?
                .into_iter()
                .map(|slot| Label::node(slot.target.as_str()))
                .collect();
            output.push(PendingInstruction::Select(
                node.kind.clone(),
                configuration(),
                targets,
                next().map(|target| Label::node(&target)),
            ));
        }
        WorkflowNodeKind::Parallel | WorkflowNodeKind::Race => {
            let targets = target_slots(node)?
                .into_iter()
                .map(|slot| Label::node(slot.target.as_str()))
                .collect();
            output.push(PendingInstruction::Fork(targets, node.id.clone()));
        }
        WorkflowNodeKind::Join => {
            output.push(PendingInstruction::Instruction(WorkflowInstruction::Join {
                join_key: node.id.clone(),
            }))
        }
    }
    Ok(())
}

fn durable(
    node: &WorkflowNode,
    kind: &str,
    input: Value,
    output: &mut Vec<PendingInstruction>,
    next: Option<String>,
) {
    output.push(PendingInstruction::Instruction(
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Coordination {
                kind: kind.to_string(),
                input,
            },
        },
    ));
    if let Some(next) = next {
        output.push(PendingInstruction::Jump(Label::node(&next)));
    } else if !matches!(node.kind, WorkflowNodeKind::End | WorkflowNodeKind::Fail) {
        output.push(PendingInstruction::Instruction(WorkflowInstruction::Return));
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

        // This is both the compiler golden and the persisted-module wire contract.  In
        // particular, the forward graph edge is fixed up to the first instruction in `end`, not
        // to the next instruction emitted while lowering `start`.
        assert_eq!(
            serde_json::to_string(&module).unwrap(),
            r#"{"version":1,"instructions":[{"op":"enter_node","node_id":"start"},{"op":"jump","target":2},{"op":"enter_node","node_id":"end"},{"op":"return"}],"source_map":[{"version":1,"instruction_start":0,"instruction_end":2,"node_id":"start"},{"version":1,"instruction_start":2,"instruction_end":4,"node_id":"end"}]}"#
        );
        let decoded: WorkflowModule =
            serde_json::from_str(&serde_json::to_string(&module).unwrap()).unwrap();
        assert_eq!(decoded, module);
    }

    #[test]
    fn fixes_a_terminal_failure_edge_to_the_fail_block() {
        let definition = WorkflowDefinition {
            id: None,
            name: "failure".into(),
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    node("start", WorkflowNodeKind::Start, Some("fail")),
                    node("fail", WorkflowNodeKind::Fail, None),
                    node("end", WorkflowNodeKind::End, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };

        let module = compile_workflow_module(&definition).unwrap();
        assert!(matches!(
            module.instructions[1],
            WorkflowInstruction::Jump { target: 2 }
        ));
        assert!(matches!(
            module.instructions[3],
            WorkflowInstruction::Fail { .. }
        ));
        assert_eq!(module.graph_location(3).unwrap().node_id, "fail");
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
