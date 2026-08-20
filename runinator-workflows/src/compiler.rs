//! Validated workflow graph to versioned workflow bytecode lowering.

use std::collections::HashMap;

use runinator_compute::{
    CallableCatalog, WorkflowValidationError, assemble_module, parse_expression,
};
use runinator_models::{
    invocation::{InvocationInstruction, InvocationModule, InvocationProgram},
    value::Value,
    workflow_ast::{ComputeProgram, ComputeStmt},
    workflow_vm::{
        WorkflowEffectRequest, WorkflowInstruction, WorkflowModule, WorkflowOutputArtifact,
        WorkflowSourceMapEntry, WorkflowVmBranch,
    },
    workflows::{
        WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowSubflowType,
    },
};

use crate::{
    parse_approval_parameters, parse_gate_parameters, parse_input_parameters,
    parse_signal_parameters, parse_switch_parameters, parse_wait_parameters, target_slots,
    validate_workflow,
};

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
                        retry: node.retry.clone(),
                        tags: action.tags.clone(),
                        required_labels: action.required_labels.clone(),
                        idempotency_key: action.idempotency_key.clone(),
                        function_binding: action.function_binding.clone(),
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
        WorkflowNodeKind::Approval => {
            let approval = parse_approval_parameters(node);
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::Approval {
                        // The approval type and metadata are intentionally embedded in the prompt
                        // envelope so the host never has to re-read the mutable authoring node.
                        prompt: serde_json::json!({
                            "approval_type": approval.approval_type,
                            "prompt": approval.prompt,
                            "metadata": approval.metadata,
                        })
                        .into(),
                        expires_at: None,
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Gate => {
            let gate = parse_gate_parameters(node);
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::Gate {
                        kind: gate.kind,
                        condition: gate.condition,
                        poll_interval_seconds: gate.poll_interval_seconds,
                        deadline_seconds: gate.deadline_seconds,
                        continue_on_timeout: matches!(
                            gate.timeout_policy,
                            crate::GateTimeoutPolicy::Continue
                        ),
                        label: gate.label,
                        metadata: gate.metadata,
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Signal => {
            let signal = parse_signal_parameters(node);
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::Signal {
                        key: signal.name,
                        filter: serde_json::to_value(signal.correlation_key)
                            .ok()
                            .map(Value::from),
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Input => {
            let input = parse_input_parameters(node);
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::Input {
                        prompt: input.prompt,
                        schema: node.parameters.clone().into(),
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Subflow => {
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::ChildRun {
                        workflow_id: node.subflow_id,
                        workflow_name: node.subflow.workflow_name.clone(),
                        input: node.parameters.clone().into(),
                        wait: matches!(node.subflow.subflow_type, WorkflowSubflowType::Wait),
                        reuse_open_run: node.subflow.reuse_open_run,
                        run_name: node.subflow.run_name.clone(),
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Invocation => {
            // Invocation already has a compiled compute module in authoring data.  Keep it as an
            // executable instruction; wrapping it in a coordination payload would force the host
            // back into the old invocation-call subsystem.
            let mut invocation = crate::parse_invocation_parameters(node)?;
            // Invocation calls inherit the node deadline unless their authored `with { timeout }
            // policy is more specific. Once it is written into the module, resuming an old run
            // cannot pick up a later edit to the node timeout.
            apply_invocation_timeout(
                &mut invocation.module,
                invocation.timeout_seconds.or(node.timeout_seconds),
            );
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Evaluate {
                    module: invocation.module,
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Audit => durable(node, "audit", configuration(), output, next()),
        WorkflowNodeKind::Checkpoint => {
            durable(node, "checkpoint", configuration(), output, next())
        }
        WorkflowNodeKind::Mutex => durable(node, "mutex", configuration(), output, next()),
        WorkflowNodeKind::Throttle => durable(node, "throttle", configuration(), output, next()),
        WorkflowNodeKind::Cooldown => durable(node, "cooldown", configuration(), output, next()),
        WorkflowNodeKind::AwaitRun => {
            let workflow = node
                .parameters
                .get("workflow")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let key = node.parameters.get("key").cloned();
            let run_id = node.parameters.get("run_id").cloned();
            let mode = node
                .parameters
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("all")
                .to_string();
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::AwaitRun {
                        workflow,
                        key,
                        run_id,
                        mode,
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Debounce => durable(node, "debounce", configuration(), output, next()),
        WorkflowNodeKind::Collect => durable(node, "collect", configuration(), output, next()),
        WorkflowNodeKind::Barrier => durable(node, "barrier", configuration(), output, next()),
        WorkflowNodeKind::CircuitBreaker => {
            durable(node, "circuit_breaker", configuration(), output, next())
        }
        WorkflowNodeKind::EventSource => {
            let event_type = node
                .parameters
                .get("event_type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let filter = node.parameters.get("filter").cloned();
            let max_events = node
                .parameters
                .get("max")
                .and_then(Value::as_i64)
                .and_then(|value| u64::try_from(value).ok())
                .filter(|value| *value > 0);
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::EventWait {
                        event_type,
                        filter,
                        max_events,
                    },
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Output => {
            // The output payload is a pure expression. Artifact publication and automation events
            // are host effects in a later phase; this instruction establishes the durable VM's
            // workflow output without retaining the old output-node dispatcher.
            let output_parameters = crate::parse_output_parameters(node)?;
            let data = serde_json::to_value(output_parameters.data)
                .map(Value::from)
                .unwrap_or(Value::Null);
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Evaluate {
                    module: expression_module(data)?,
                },
            ));
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::SetOutput {
                    event_type: output_parameters.event_type,
                    artifacts: output_parameters
                        .items
                        .into_iter()
                        .map(|item| {
                            let source = serde_json::to_value(item.source)
                                .map(Value::from)
                                .unwrap_or(Value::Null);
                            Ok(WorkflowOutputArtifact {
                                name: item.name,
                                source: expression_module(source)?,
                            })
                        })
                        .collect::<Result<Vec<_>, WorkflowValidationError>>()?,
                },
            ));
            jump_next(output);
        }
        WorkflowNodeKind::Config | WorkflowNodeKind::Assert | WorkflowNodeKind::Transform => {
            // These nodes are pure computations. Compile their JSON expression trees now, while
            // definitions are validated, rather than leaving a generic node payload for the host
            // to interpret at run time.
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Evaluate {
                    module: expression_module(node.parameters.clone().into())?,
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
        WorkflowNodeKind::Switch => {
            // A switch is just an ordered collection of authoring conditions. Resolve its target
            // labels here so the VM executes the same branch opcode as a condition node instead
            // of retaining a reducer-era selector payload.
            let switch = parse_switch_parameters(node)?;
            output.push(PendingInstruction::Branch(
                switch
                    .cases
                    .into_iter()
                    .map(|case| (case.condition, Label::node(case.target.as_str())))
                    .collect(),
                switch
                    .default
                    .as_ref()
                    .map(|target| Label::node(target.as_str()))
                    .or_else(|| next().map(|target| Label::node(&target))),
            ));
        }
        WorkflowNodeKind::Toggle
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

/// Turn the JSON expression tree carried by a pure graph node into invocation bytecode.  The
/// builtin catalog includes the compiler-only operator intrinsics used by `assemble_module`; any
/// provider call remains a typed durable call in the invocation module rather than an untyped JSON
/// callback.
fn expression_module(
    value: Value,
) -> Result<runinator_models::invocation::InvocationModule, WorkflowValidationError> {
    let expression = parse_expression(&value)?;
    assemble_module(
        &ComputeProgram(vec![ComputeStmt::Return(expression)]),
        &[],
        &CallableCatalog::builtin(),
    )
}

fn apply_invocation_timeout(module: &mut InvocationModule, timeout_seconds: Option<i64>) {
    let Some(timeout_seconds) = timeout_seconds.filter(|timeout| *timeout > 0) else {
        return;
    };
    apply_program_timeout(&mut module.entry, timeout_seconds);
    for function in &mut module.functions {
        apply_program_timeout(&mut function.body, timeout_seconds);
    }
}

fn apply_program_timeout(program: &mut InvocationProgram, timeout_seconds: i64) {
    for instruction in &mut program.instructions {
        match instruction {
            InvocationInstruction::Call { policy, .. } => {
                policy
                    .get_or_insert_default()
                    .timeout_seconds
                    .get_or_insert(timeout_seconds);
            }
            InvocationInstruction::Closure { body, .. } => {
                apply_program_timeout(body, timeout_seconds);
            }
            _ => {}
        }
    }
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
    use runinator_models::functions::FunctionBinding;
    use runinator_models::invocation::{
        CallableTarget, InvocationInstruction, InvocationModule, InvocationProgram,
    };
    use runinator_models::workflows::{
        WorkflowAction, WorkflowGraph, WorkflowRetry, WorkflowTransitions,
    };
    use std::collections::BTreeMap;

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

    #[test]
    fn action_effect_freezes_dispatch_and_idempotency_policy() {
        let mut action = WorkflowAction {
            provider: "functions".into(),
            function: "invoke".into(),
            timeout_seconds: 45,
            configuration: serde_json::from_value(serde_json::json!({"input": "hello"})).unwrap(),
            mcp_enabled: false,
            tags: vec!["critical".into()],
            required_labels: BTreeMap::from([("runner".into(), "isolated".into())]),
            idempotency_key: Some(Value::String("order-42".into())),
            function_binding: Some(FunctionBinding {
                package_id: uuid::Uuid::nil(),
                package_name: "billing".into(),
                namespace: None,
                version_id: uuid::Uuid::nil(),
                version: 7,
                export_id: uuid::Uuid::nil(),
                export_name: "charge".into(),
                artifact_digest: "sha256:abc".into(),
            }),
        };
        // Keep this construction intentionally explicit: the VM request must carry all policy
        // that used to be re-read from the graph at dispatch time.
        action.tags.push("billing".into());
        let definition = WorkflowDefinition {
            id: None,
            name: "policy".into(),
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    node("start", WorkflowNodeKind::Start, Some("call")),
                    WorkflowNode {
                        id: "call".into(),
                        kind: WorkflowNodeKind::Action,
                        action: Some(action),
                        retry: WorkflowRetry {
                            max_attempts: 3,
                            ..Default::default()
                        },
                        transitions: WorkflowTransitions {
                            next: Some(WorkflowNodeRef::new("end")),
                            ..Default::default()
                        },
                        ..node("unused", WorkflowNodeKind::Action, None)
                    },
                    node("end", WorkflowNodeKind::End, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };

        let module = compile_workflow_module(&definition).unwrap();
        let request = module
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                WorkflowInstruction::Effect {
                    request:
                        WorkflowEffectRequest::Action {
                            provider,
                            function,
                            input,
                            timeout_seconds,
                            retry,
                            tags,
                            required_labels,
                            idempotency_key,
                            function_binding,
                        },
                } => Some((
                    provider,
                    function,
                    input,
                    timeout_seconds,
                    retry,
                    tags,
                    required_labels,
                    idempotency_key,
                    function_binding,
                )),
                _ => None,
            })
            .expect("action node produces an effect");
        assert_eq!(request.0, "functions");
        assert_eq!(request.1, "invoke");
        assert_eq!(request.2.get("input"), Some(&Value::String("hello".into())));
        assert_eq!(*request.3, Some(45));
        assert_eq!(request.4.max_attempts, 3);
        assert_eq!(request.5, &vec!["critical", "billing"]);
        assert_eq!(
            request.6.get("runner").map(String::as_str),
            Some("isolated")
        );
        assert_eq!(request.7, &Some(Value::String("order-42".into())));
        assert_eq!(
            request
                .8
                .as_ref()
                .map(FunctionBinding::call_path)
                .as_deref(),
            Some("functions.billing.charge")
        );
    }

    #[test]
    fn switch_lowers_to_ordered_branch_instead_of_selector_payload() {
        let mut switch = node("switch", WorkflowNodeKind::Switch, None);
        switch.parameters = serde_json::from_value(serde_json::json!({
            "value": "kind",
            "cases": [{"equals": "approved", "target": {"$node": "end"}}],
            "default": {"$node": "fail"}
        }))
        .unwrap();
        let definition = WorkflowDefinition {
            id: None,
            name: "switch".into(),
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    node("start", WorkflowNodeKind::Start, Some("switch")),
                    switch,
                    node("end", WorkflowNodeKind::End, None),
                    node("fail", WorkflowNodeKind::Fail, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };

        let module = compile_workflow_module(&definition).unwrap();
        assert!(
            module
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, WorkflowInstruction::Branch { .. }))
        );
        assert!(
            !module
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, WorkflowInstruction::Select { .. }))
        );
    }

    #[test]
    fn compute_invocation_and_output_nodes_lower_without_json_placeholders() {
        let mut config = node("config", WorkflowNodeKind::Config, None);
        config.parameters = serde_json::from_value(serde_json::json!({"name": "renamed"})).unwrap();
        let mut transform = node("transform", WorkflowNodeKind::Transform, None);
        transform.parameters = serde_json::from_value(serde_json::json!({
            "bindings": {"total": {"$add": [1, 2]}}
        }))
        .unwrap();
        let mut assertion = node("assert", WorkflowNodeKind::Assert, None);
        assertion.parameters =
            serde_json::from_value(serde_json::json!({"assertions": []})).unwrap();
        let mut invocation = node("invoke", WorkflowNodeKind::Invocation, None);
        invocation.parameters = serde_json::from_value(serde_json::json!({
            "module": InvocationModule::new(InvocationProgram::new(vec![
                InvocationInstruction::Call {
                    target: CallableTarget::Provider {
                        provider: "demo".into(),
                        function: "run".into(),
                    },
                    argc: 0,
                    names: Vec::new(),
                    policy: None,
                },
                InvocationInstruction::Return,
            ])),
            "timeout_seconds": 17
        }))
        .unwrap();
        let mut output_node = node("output", WorkflowNodeKind::Output, None);
        output_node.parameters = serde_json::from_value(serde_json::json!({
            "event_type": "published",
            "data": {"result": "ok"},
            "items": [{"name": "report", "source": {"id": "artifact-1"}}]
        }))
        .unwrap();

        for node in [&config, &transform, &assertion, &invocation] {
            let mut instructions = Vec::new();
            lower_node(node, &mut instructions).unwrap();
            assert!(matches!(
                instructions.first(),
                Some(PendingInstruction::Instruction(
                    WorkflowInstruction::Evaluate { .. }
                ))
            ));
            assert!(!instructions.iter().any(|instruction| matches!(
                instruction,
                PendingInstruction::Instruction(WorkflowInstruction::PureNode { .. })
            )));
        }

        let mut instructions = Vec::new();
        lower_node(&invocation, &mut instructions).unwrap();
        let PendingInstruction::Instruction(WorkflowInstruction::Evaluate { module }) =
            &instructions[0]
        else {
            panic!("invocation must lower to evaluate");
        };
        assert!(matches!(
            module.entry.instructions.first(),
            Some(InvocationInstruction::Call {
                policy: Some(policy),
                ..
            }) if policy.timeout_seconds == Some(17)
        ));

        let mut instructions = Vec::new();
        lower_node(&output_node, &mut instructions).unwrap();
        assert!(matches!(
            instructions.as_slice(),
            [
                PendingInstruction::Instruction(WorkflowInstruction::Evaluate { .. }),
                PendingInstruction::Instruction(WorkflowInstruction::SetOutput {
                    event_type: Some(event_type),
                    artifacts,
                }),
            ] if event_type == "published" && artifacts.len() == 1 && artifacts[0].name == "report"
        ));

        // Exercise the public compiler as well: the Phase 4 node kinds must coexist in one
        // linear graph, keep their source-map ranges, and leave exactly one provider effect.
        let mut nodes = vec![config, transform, assertion, invocation, output_node];
        let ids = nodes.iter().map(|node| node.id.clone()).collect::<Vec<_>>();
        for (node, successor) in nodes.iter_mut().zip(ids.iter().skip(1)) {
            node.transitions.next = Some(WorkflowNodeRef::new(successor));
        }
        let mut action = node("action", WorkflowNodeKind::Action, Some("end"));
        action.action = Some(WorkflowAction {
            provider: "std".into(),
            function: "log".into(),
            timeout_seconds: 10,
            configuration: serde_json::from_value(serde_json::json!({"message": "done"})).unwrap(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: BTreeMap::new(),
            idempotency_key: None,
            function_binding: None,
        });
        nodes.last_mut().unwrap().transitions.next = Some(WorkflowNodeRef::new("action"));
        let mut graph_nodes = vec![node("start", WorkflowNodeKind::Start, Some("config"))];
        graph_nodes.extend(nodes);
        graph_nodes.push(action);
        graph_nodes.push(node("end", WorkflowNodeKind::End, None));
        let definition = WorkflowDefinition {
            id: None,
            name: "phase-four".into(),
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: graph_nodes,
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };
        let module = compile_workflow_module(&definition).unwrap();
        assert_eq!(
            module
                .instructions
                .iter()
                .filter(|instruction| matches!(instruction, WorkflowInstruction::Evaluate { .. }))
                .count(),
            5
        );
        assert_eq!(
            module
                .instructions
                .iter()
                .filter(|instruction| matches!(
                    instruction,
                    WorkflowInstruction::Effect {
                        request: WorkflowEffectRequest::Action { .. }
                    }
                ))
                .count(),
            1
        );
        let decoded: WorkflowModule =
            serde_json::from_str(&serde_json::to_string(&module).unwrap()).unwrap();
        assert_eq!(decoded, module);
    }

    #[test]
    fn parking_nodes_lower_to_typed_effects_without_coordination_placeholders() {
        let mut wait = node("wait", WorkflowNodeKind::Wait, None);
        wait.wait.seconds = Some(runinator_models::workflows::WorkflowWaitSeconds::Integer(5));

        let mut approval = node("approval", WorkflowNodeKind::Approval, None);
        approval.parameters = serde_json::from_value(serde_json::json!({
            "approval_type": "release",
            "prompt": "Ship this build?",
        }))
        .unwrap();

        let mut gate = node("gate", WorkflowNodeKind::Gate, None);
        gate.parameters = serde_json::from_value(serde_json::json!({
            "kind": "manual",
            "poll_interval": 15,
            "timeout": 90,
            "timeout_policy": "continue",
            "label": "production",
        }))
        .unwrap();

        let mut signal = node("signal", WorkflowNodeKind::Signal, None);
        signal.parameters = serde_json::from_value(serde_json::json!({
            "name": "release-ready",
            "correlation_key": {"$ref": "input.release_id"},
        }))
        .unwrap();

        let mut input = node("input", WorkflowNodeKind::Input, None);
        input.parameters =
            serde_json::from_value(serde_json::json!({"prompt": "Version"})).unwrap();

        let mut event = node("event", WorkflowNodeKind::EventSource, None);
        event.parameters = serde_json::from_value(serde_json::json!({
            "event_type": "build.finished",
            "filter": {"branch": "main"},
            "max": 1,
        }))
        .unwrap();

        let mut subflow = node("subflow", WorkflowNodeKind::Subflow, None);
        subflow.subflow_id = Some(uuid::Uuid::nil());
        subflow.subflow.subflow_type = WorkflowSubflowType::Wait;
        subflow.parameters = serde_json::from_value(serde_json::json!({"release": "42"})).unwrap();

        let mut await_run = node("await", WorkflowNodeKind::AwaitRun, None);
        await_run.parameters = serde_json::from_value(serde_json::json!({
            "workflow": "child",
            "key": "release-42",
            "mode": "any",
        }))
        .unwrap();

        let expected = [
            WorkflowNodeKind::Wait,
            WorkflowNodeKind::Approval,
            WorkflowNodeKind::Gate,
            WorkflowNodeKind::Signal,
            WorkflowNodeKind::Input,
            WorkflowNodeKind::EventSource,
            WorkflowNodeKind::Subflow,
            WorkflowNodeKind::AwaitRun,
        ];
        let nodes = [
            &wait, &approval, &gate, &signal, &input, &event, &subflow, &await_run,
        ];

        for (node, kind) in nodes.into_iter().zip(expected) {
            let mut instructions = Vec::new();
            lower_node(node, &mut instructions).unwrap();
            let Some(PendingInstruction::Instruction(WorkflowInstruction::Effect { request })) =
                instructions.first()
            else {
                panic!("{kind:?} must start with one parking effect");
            };
            assert!(
                !matches!(request, WorkflowEffectRequest::Coordination { .. }),
                "{kind:?} must not fall back to an untyped coordination effect"
            );
            let correct_request = match (kind.clone(), request) {
                (WorkflowNodeKind::Wait, WorkflowEffectRequest::TimerDelay { seconds: 5 })
                | (WorkflowNodeKind::Approval, WorkflowEffectRequest::Approval { .. })
                | (WorkflowNodeKind::Gate, WorkflowEffectRequest::Gate { .. })
                | (WorkflowNodeKind::Signal, WorkflowEffectRequest::Signal { .. })
                | (WorkflowNodeKind::Input, WorkflowEffectRequest::Input { .. })
                | (WorkflowNodeKind::EventSource, WorkflowEffectRequest::EventWait { .. })
                | (WorkflowNodeKind::Subflow, WorkflowEffectRequest::ChildRun { wait: true, .. }) => {
                    true
                }
                (WorkflowNodeKind::AwaitRun, WorkflowEffectRequest::AwaitRun { mode, .. }) => {
                    mode == "any"
                }
                _ => false,
            };
            assert!(
                correct_request,
                "{kind:?} lowered to the wrong parking request"
            );
        }
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
