//! Validated workflow graph to versioned workflow bytecode lowering.

use std::collections::HashMap;

use runinator_compute::{
    CallableCatalog, WorkflowValidationError, assemble_module, parse_expression,
};
use runinator_models::{
    interrupt::InterruptSource,
    invocation::{InvocationInstruction, InvocationModule, InvocationProgram},
    value::Value,
    workflow_ast::{ComputeProgram, ComputeStmt},
    workflow_vm::{
        WorkflowBranchPolicy, WorkflowEffectRequest, WorkflowInstruction, WorkflowModule,
        WorkflowOutputArtifact, WorkflowSourceMapEntry, WorkflowVmBranch,
        WorkflowVmInterruptHandler,
    },
    workflows::{
        WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowSubflowType,
    },
};

use crate::node_kinds::graph_role;
use crate::{
    BranchPolicy, interrupt_declarations, parse_approval_parameters, parse_gate_parameters,
    parse_input_parameters, parse_join_parameters, parse_map_parameters, parse_parallel_parameters,
    parse_race_parameters, parse_signal_parameters, parse_switch_parameters, parse_try_parameters,
    parse_wait_parameters, target_slots, validate_workflow,
};

/// A symbolic basic-block address.  Keeping these until the final layout pass means graph
/// traversal never needs to guess an instruction offset while it is lowering a node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Label(String);

impl Label {
    fn node(node_id: &str) -> Self {
        Self(node_id.to_owned())
    }

    /// A synthetic block owned by `node_id`. The `#` prefix cannot collide with a graph node id,
    /// which validation restricts to identifier characters.
    fn synthetic(node_id: &str, suffix: &str) -> Self {
        Self(format!("{node_id}#{suffix}"))
    }

    /// The edge slot a synthetic block stands for (`on_failure`, `on_timeout`, ...), or `None` for
    /// a node's own block. This is what tells an operator watching a cursor which edge it took.
    fn edge_slot(&self) -> Option<&str> {
        self.0.split_once('#').map(|(_, slot)| slot)
    }
}

/// An instruction before its graph targets have been laid out.
#[allow(
    clippy::large_enum_variant,
    reason = "this short-lived compiler representation is consumed before module serialization and direct variants keep lowering straightforward"
)]
enum PendingInstruction {
    Instruction(WorkflowInstruction),
    Jump(Label),
    Branch(
        Vec<(runinator_models::workflows::WorkflowCondition, Label)>,
        Option<Label>,
    ),
    Select(WorkflowNodeKind, Value, Vec<Label>, Option<Label>),
    Fork(Vec<Label>, String),
    BeginLoop {
        loop_key: String,
        body: Label,
        exit: Label,
        max_iterations: Option<u64>,
    },
    CheckInterrupt(Vec<(InterruptSource, Option<String>, Option<i64>, Label)>),
    BeginTry {
        try_key: String,
        catch: Option<Label>,
        on_timeout: Option<Label>,
        on_reject: Option<Label>,
        finally: Option<Label>,
    },
    Race {
        targets: Vec<Label>,
        race_key: String,
        winner: WorkflowBranchPolicy,
    },
    BeginMap {
        map_key: String,
        body: Label,
        exit: Label,
        concurrency: u64,
    },
    Reenter {
        reentry_key: String,
        target: Label,
        exhausted: Option<Label>,
        max_visits: u64,
    },
}

/// A node-owned basic block.  The block boundary is also the unit recorded in the module's
/// source map, so every instruction generated for a graph node has one stable graph location.
struct BasicBlock {
    label: Label,
    node_id: String,
    instructions: Vec<PendingInstruction>,
    /// Whether an interrupt may suspend a thread positioned here.
    interruptible: bool,
    /// Offset within the block of its trailing exit sequence, when it has one.
    exit_offset: Option<usize>,
}

/// Compile a validated authoring graph into an immutable module with a mandatory graph source map.
pub fn compile_workflow_module(
    workflow: &WorkflowDefinition,
) -> Result<WorkflowModule, WorkflowValidationError> {
    let (start, nodes) = validate_workflow(workflow)?;
    // Handlers are frozen into the module. An unknown or disabled source is dropped here rather
    // than at runtime, so an old binary reading a newer definition simply has fewer handlers
    // instead of failing the compile. Timer identity belongs to the declaration, not its target:
    // two intervals may deliberately share one handler region.
    let mut timer_index = 0usize;
    let declared_handlers: Vec<(InterruptSource, Option<String>, Option<i64>, Label)> =
        interrupt_declarations(workflow, &nodes)
            .into_iter()
            .filter(|declaration| declaration.enabled)
            .filter_map(|declaration| {
                let source = declaration.source()?;
                let timer_id = if source == InterruptSource::Timer {
                    let id = format!("timer:{timer_index}");
                    timer_index += 1;
                    Some(id)
                } else {
                    None
                };
                Some((
                    source,
                    timer_id,
                    declaration.interval_seconds,
                    Label::node(&declaration.handler),
                ))
            })
            .collect();
    let mut ordered = Vec::with_capacity(nodes.len());
    let start_node = nodes
        .iter()
        .find(|node| node.id == start)
        .expect("validation proved the start node exists");
    ordered.push(start_node);
    ordered.extend(nodes.iter().filter(|node| node.id != start));

    let mut blocks = Vec::with_capacity(ordered.len());
    for node in ordered {
        let mut instructions = vec![
            PendingInstruction::Instruction(WorkflowInstruction::DebugBoundary {
                label: Some(node.id.clone()),
            }),
            PendingInstruction::Instruction(WorkflowInstruction::EnterNode {
                node_id: node.id.clone(),
            }),
        ];
        if node.reentry.enabled {
            let max_visits = u64::try_from(node.reentry.max_visits).map_err(|_| {
                WorkflowValidationError::InvalidReentry(format!(
                    "node '{}' has a negative max_visits",
                    node.id
                ))
            })?;
            instructions.push(PendingInstruction::Reenter {
                reentry_key: node.id.clone(),
                // Re-entering a graph node must continue at its first real operation, after the
                // source-map entry marker and the guard itself.
                target: Label::node(&node.id),
                exhausted: node
                    .reentry
                    .on_exhausted
                    .as_ref()
                    .map(|target| Label::node(target.as_str())),
                max_visits,
            });
        }
        let interruptible = graph_role(&node.kind).interruptible;
        // the safe point an externally requested interrupt fires at. emitted only where one could
        // actually be serviced, so a non-interruptible node costs nothing.
        if interruptible && !declared_handlers.is_empty() {
            instructions.push(PendingInstruction::CheckInterrupt(
                declared_handlers.clone(),
            ));
        }
        let mut body = Vec::new();
        lower_node(node, &mut body)?;
        let guard = apply_failure_edges(node, &mut body);
        instructions.extend(body);
        let exit_offset = exit_offset(&instructions);
        blocks.push(BasicBlock {
            label: Label::node(&node.id),
            node_id: node.id.clone(),
            instructions,
            interruptible,
            exit_offset,
        });
        // one landing block per declared edge: it closes the node's guard frame before handing
        // control to the target, so a re-entered node opens a fresh guard instead of finding the
        // previous visit's.
        for (label, target) in guard {
            blocks.push(BasicBlock {
                label,
                node_id: node.id.clone(),
                instructions: vec![
                    PendingInstruction::Instruction(WorkflowInstruction::EndTry {
                        try_key: guard_key(&node.id),
                    }),
                    PendingInstruction::Jump(target),
                ],
                interruptible: false,
                exit_offset: None,
            });
        }
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
        ranges.push((
            begin,
            instruction_count,
            block.node_id.clone(),
            block.label.edge_slot().map(str::to_owned),
            block.interruptible,
            block.exit_offset.map(|offset| begin + offset),
        ));
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
                        .map(&resolve)
                        .collect::<Result<Vec<_>, _>>()?,
                    default: default.as_ref().map(resolve).transpose()?,
                }
            }
            PendingInstruction::Fork(targets, join_key) => WorkflowInstruction::Fork {
                targets: targets
                    .iter()
                    .map(&resolve)
                    .collect::<Result<Vec<_>, _>>()?,
                join_key,
            },
            PendingInstruction::BeginLoop {
                loop_key,
                body,
                exit,
                max_iterations,
            } => WorkflowInstruction::BeginLoop {
                loop_key,
                body: resolve(&body)?,
                exit: resolve(&exit)?,
                max_iterations,
            },
            PendingInstruction::CheckInterrupt(handlers) => WorkflowInstruction::CheckInterrupt {
                handlers: handlers
                    .iter()
                    .map(|(source, timer_id, interval_seconds, label)| {
                        Ok(WorkflowVmInterruptHandler {
                            source: *source,
                            target: resolve(label)?,
                            timer_id: timer_id.clone(),
                            interval_seconds: *interval_seconds,
                        })
                    })
                    .collect::<Result<Vec<_>, WorkflowValidationError>>()?,
            },
            PendingInstruction::BeginTry {
                try_key,
                catch,
                on_timeout,
                on_reject,
                finally,
            } => WorkflowInstruction::BeginTry {
                try_key,
                catch: catch.as_ref().map(resolve).transpose()?,
                on_timeout: on_timeout.as_ref().map(resolve).transpose()?,
                on_reject: on_reject.as_ref().map(resolve).transpose()?,
                finally: finally.as_ref().map(resolve).transpose()?,
            },
            PendingInstruction::Race {
                targets,
                race_key,
                winner,
            } => WorkflowInstruction::Race {
                targets: targets
                    .iter()
                    .map(&resolve)
                    .collect::<Result<Vec<_>, _>>()?,
                race_key,
                winner,
            },
            PendingInstruction::BeginMap {
                map_key,
                body,
                exit,
                concurrency,
            } => WorkflowInstruction::BeginMap {
                map_key,
                body: resolve(&body)?,
                exit: resolve(&exit)?,
                concurrency,
            },
            PendingInstruction::Reenter {
                reentry_key,
                target,
                exhausted,
                max_visits,
            } => {
                WorkflowInstruction::Reenter {
                    reentry_key,
                    // The instruction itself treats this target as the continuation point after
                    // the debug boundary, node-entry marker, and guard. The symbolic label
                    // preserves graph validation and source-map ownership even though the offset
                    // is finalized by the VM.
                    target: resolve(&target)? + 3,
                    exhausted: exhausted.as_ref().map(resolve).transpose()?,
                    max_visits,
                }
            }
        });
    }
    let interrupt_handlers = declared_handlers
        .iter()
        .map(|(source, timer_id, interval_seconds, label)| {
            Ok(WorkflowVmInterruptHandler {
                source: *source,
                target: resolve(label)?,
                timer_id: timer_id.clone(),
                interval_seconds: *interval_seconds,
            })
        })
        .collect::<Result<Vec<_>, WorkflowValidationError>>()?;
    Ok(WorkflowModule {
        version: runinator_models::workflow_vm::WORKFLOW_VM_VERSION,
        instructions,
        source_map: ranges
            .into_iter()
            .map(
                |(
                    instruction_start,
                    instruction_end,
                    node_id,
                    edge_label,
                    interruptible,
                    exit_instruction_pointer,
                )| WorkflowSourceMapEntry {
                    version: runinator_models::workflow_vm::WORKFLOW_SOURCE_MAP_VERSION,
                    instruction_start,
                    instruction_end,
                    node_id,
                    edge_label,
                    interruptible,
                    exit_instruction_pointer,
                },
            )
            .collect(),
        interrupt_handlers,
    })
}

/// Where a block hands control on: the first instruction of its trailing exit sequence.
///
/// An interrupt handler answering `continue` jumps here, so it must be *before* the guard's
/// `EndTry` — landing on the jump alone would leave the node's failure-edge frame open.
fn exit_offset(instructions: &[PendingInstruction]) -> Option<usize> {
    let mut offset = instructions.len();
    if offset > 0 && matches!(instructions[offset - 1], PendingInstruction::Jump(_)) {
        offset -= 1;
    } else {
        return None;
    }
    if offset > 0
        && matches!(
            instructions[offset - 1],
            PendingInstruction::Instruction(WorkflowInstruction::EndTry { .. })
        )
    {
        offset -= 1;
    }
    Some(offset)
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
    let parameters = || node.parameters.clone().into();
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
            // terminates a handler region. it does not transition anywhere in this thread: the
            // handler continuation retires here and its mode is what moves the thread it froze.
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::ResumeInterrupt {
                    mode: node
                        .parameters
                        .get("mode")
                        .and_then(Value::as_str)
                        .and_then(|mode| mode.parse().ok())
                        .unwrap_or_default(),
                },
            ));
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
                        workspace_affinity: action.workspace_affinity.clone(),
                        execution_profile: action.execution_profile.clone(),
                        idempotency_key: action.idempotency_key.clone(),
                        function_binding: action.function_binding.clone(),
                    },
                },
            ));
            if let Some(compensation) = &node.compensation {
                output.push(PendingInstruction::Instruction(
                    WorkflowInstruction::RegisterCompensation {
                        compensation_key: node.id.clone(),
                        request: WorkflowEffectRequest::Action {
                            provider: compensation.provider.clone(),
                            function: compensation.function.clone(),
                            input: serde_json::to_value(&compensation.configuration)
                                .map(Value::from)
                                .unwrap_or(Value::Null),
                            timeout_seconds: Some(compensation.timeout_seconds),
                            retry: node.retry.clone(),
                            tags: compensation.tags.clone(),
                            required_labels: compensation.required_labels.clone(),
                            workspace_affinity: compensation.workspace_affinity.clone(),
                            execution_profile: compensation.execution_profile.clone(),
                            idempotency_key: compensation.idempotency_key.clone(),
                            function_binding: compensation.function_binding.clone(),
                        },
                    },
                ));
            }
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
            let revision_pin = node
                .subflow
                .target
                .as_ref()
                .and_then(|reference| reference.revision_pin.as_ref());
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Effect {
                    request: WorkflowEffectRequest::ChildRun {
                        workflow_id: node.subflow.target_workflow_id().or(node.subflow_id),
                        workflow_name: node.subflow.workflow_name.clone(),
                        workflow_revision: revision_pin.map(|pin| pin.revision),
                        workflow_revision_digest: revision_pin.map(|pin| pin.digest.clone()),
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
        WorkflowNodeKind::Audit => durable(node, "audit", parameters(), output, next()),
        WorkflowNodeKind::Checkpoint => durable(node, "checkpoint", parameters(), output, next()),
        WorkflowNodeKind::Mutex => {
            // The standard node timeout is the wait-to-acquire budget for a mutex. Coordination
            // effects otherwise carry only their resolved parameters, so freeze this graph-level
            // policy in the durable request explicitly.
            let mut input = parameters();
            if let (Some(timeout_seconds), Value::Object(parameters)) =
                (node.timeout_seconds, &mut input)
            {
                parameters.insert("timeout_seconds".into(), Value::from(timeout_seconds));
            }
            durable(node, "mutex", input, output, next())
        }
        WorkflowNodeKind::Throttle => durable(node, "throttle", parameters(), output, next()),
        WorkflowNodeKind::Cooldown => durable(node, "cooldown", parameters(), output, next()),
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
        WorkflowNodeKind::Debounce => durable(node, "debounce", parameters(), output, next()),
        WorkflowNodeKind::Collect => durable(node, "collect", parameters(), output, next()),
        WorkflowNodeKind::Barrier => durable(node, "barrier", parameters(), output, next()),
        WorkflowNodeKind::CircuitBreaker => {
            durable(node, "circuit_breaker", parameters(), output, next())
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
        WorkflowNodeKind::Loop => {
            let items = node.parameters.get("items").cloned().ok_or_else(|| {
                WorkflowValidationError::InvalidNodeParameters {
                    node: node.id.clone(),
                    message: "loop.items is required".into(),
                }
            })?;
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Evaluate {
                    module: expression_module(items)?,
                },
            ));
            let body = node.transitions.next.as_ref().ok_or_else(|| {
                WorkflowValidationError::MissingTransition {
                    node: node.id.clone(),
                    target: "next".into(),
                }
            })?;
            let exit = node
                .transitions
                .on_success
                .as_ref()
                .or(node.transitions.next.as_ref())
                .unwrap();
            output.push(PendingInstruction::BeginLoop {
                loop_key: node.id.clone(),
                body: Label::node(body.as_str()),
                exit: Label::node(exit.as_str()),
                max_iterations: node
                    .max_iterations
                    .and_then(|value| u64::try_from(value).ok()),
            });
        }
        WorkflowNodeKind::Try => {
            let params = parse_try_parameters(node)?;
            output.push(PendingInstruction::BeginTry {
                try_key: node.id.clone(),
                catch: params
                    .catch
                    .as_ref()
                    .map(|target| Label::node(target.as_str())),
                on_timeout: None,
                on_reject: None,
                finally: params
                    .finally
                    .as_ref()
                    .map(|target| Label::node(target.as_str())),
            });
            output.push(PendingInstruction::Jump(Label::node(params.body.as_str())));
            // A body/catch/finally returns to this node. On that second entry BeginTry consumes
            // the frame and routes here instead of starting a new protected region.
            if let Some(target) = next() {
                output.push(PendingInstruction::Jump(Label::node(&target)));
            } else {
                output.push(PendingInstruction::Instruction(WorkflowInstruction::Return));
            }
        }
        WorkflowNodeKind::Toggle | WorkflowNodeKind::Percentage => {
            let mut default = next().map(|target| Label::node(&target));
            let targets = target_slots(node)?
                .into_iter()
                .filter_map(|slot| {
                    // `percentage.default` is a fallback target, not another weighted bucket.
                    // Keep it out of `targets` so the VM can continue to require one target per
                    // bucket; otherwise a graph with an authored default always fails that
                    // invariant before it can select anything.
                    if node.kind == WorkflowNodeKind::Percentage && slot.key == "default" {
                        default = Some(Label::node(slot.target.as_str()));
                        None
                    } else {
                        Some(Label::node(slot.target.as_str()))
                    }
                })
                .collect();
            output.push(PendingInstruction::Select(
                node.kind.clone(),
                configuration(),
                targets,
                default,
            ));
        }
        WorkflowNodeKind::Parallel => {
            let parallel = parse_parallel_parameters(node)?;
            output.push(PendingInstruction::Fork(
                parallel
                    .branches
                    .iter()
                    .map(|target| Label::node(target.as_str()))
                    .collect(),
                node.id.clone(),
            ));
        }
        WorkflowNodeKind::Join => {
            let join = parse_join_parameters(node)?;
            output.push(PendingInstruction::Instruction(WorkflowInstruction::Join {
                join_key: node.id.clone(),
                expected: join.wait_for.len() as u64,
                mode: branch_policy(join.mode),
            }))
        }
        WorkflowNodeKind::Race => {
            let race = parse_race_parameters(node)?;
            // `Race` is distinct from a regular fork even though both create children: the
            // parent persists a race frame, which gives the later store transaction a durable
            // winner and a deterministic loser set to cancel.
            output.push(PendingInstruction::Race {
                targets: race
                    .branches
                    .iter()
                    .map(|target| Label::node(target.as_str()))
                    .collect(),
                race_key: node.id.clone(),
                winner: branch_policy(race.winner),
            });
        }
        WorkflowNodeKind::Map => {
            let map = parse_map_parameters(node)?;
            let items = node.parameters.get("items").cloned().ok_or_else(|| {
                WorkflowValidationError::InvalidNodeParameters {
                    node: node.id.clone(),
                    message: "map.items is required".into(),
                }
            })?;
            output.push(PendingInstruction::Instruction(
                WorkflowInstruction::Evaluate {
                    module: expression_module(items)?,
                },
            ));
            let exit = next().ok_or_else(|| WorkflowValidationError::MissingTransition {
                node: node.id.clone(),
                target: "on_success".into(),
            })?;
            output.push(PendingInstruction::BeginMap {
                map_key: node.id.clone(),
                body: Label::node(map.target.as_str()),
                exit: Label::node(&exit),
                concurrency: map.concurrency.unwrap_or(1) as u64,
            });
        }
    }
    Ok(())
}

/// The try key for a node's compiled failure edges. Distinct from the `try` node's own key, which
/// is the bare node id, so a `try` node may also carry its own `on_failure` edge.
fn guard_key(node_id: &str) -> String {
    format!("{node_id}#edge")
}

/// Does this pending instruction hand control somewhere other than the next instruction?
fn transfers_control(instruction: &PendingInstruction) -> bool {
    match instruction {
        PendingInstruction::Instruction(instruction) => matches!(
            instruction,
            WorkflowInstruction::Jump { .. }
                | WorkflowInstruction::Return
                | WorkflowInstruction::Fail { .. }
                | WorkflowInstruction::Join { .. }
        ),
        PendingInstruction::Jump(_)
        | PendingInstruction::Branch(..)
        | PendingInstruction::Select(..)
        | PendingInstruction::Fork(..)
        | PendingInstruction::Race { .. }
        | PendingInstruction::BeginLoop { .. }
        | PendingInstruction::BeginMap { .. }
        | PendingInstruction::BeginTry { .. }
        | PendingInstruction::Reenter { .. } => true,
        PendingInstruction::CheckInterrupt(_) => false,
    }
}

/// Compile a node's `on_failure` / `on_timeout` / `on_reject` edges into a guard around its body,
/// and return the synthetic landing blocks the caller must emit.
///
/// Without this the edges are authored, validated, and decompiled but never executed: a failing
/// step would unwind to the enclosing `try` (or fail the run) rather than taking the edge the
/// author wrote. The guard is only applied to a node whose lowering falls through or ends in a
/// plain jump — a node that forks, branches, or loops has no single point at which the frame could
/// be closed, and those kinds carry no failure edge in the catalog.
fn apply_failure_edges(
    node: &WorkflowNode,
    body: &mut Vec<PendingInstruction>,
) -> Vec<(Label, Label)> {
    let edges = [
        ("on_failure", node.transitions.on_failure.as_ref()),
        ("on_timeout", node.transitions.on_timeout.as_ref()),
        ("on_reject", node.transitions.on_reject.as_ref()),
    ];
    if edges.iter().all(|(_, target)| target.is_none()) {
        return Vec::new();
    }
    // a `try` node is re-entered by its own body, catch, and finally blocks, and `BeginTry`
    // resolves that second entry by consuming the frame it finds. a guard wrapped around it would
    // be the frame consumed, so the node routes its own failures through `catch`/`finally`.
    if node.kind == WorkflowNodeKind::Try {
        return Vec::new();
    }
    let tail_position = body.iter().rposition(transfers_control);
    let insert_at = match tail_position {
        Some(position) if position + 1 == body.len() => {
            if !matches!(body[position], PendingInstruction::Jump(_)) {
                return Vec::new();
            }
            position
        }
        Some(_) => return Vec::new(),
        None => body.len(),
    };
    let landing = |slot: &str| Label::synthetic(&node.id, slot);
    let mut blocks = Vec::new();
    for (slot, target) in edges {
        if let Some(target) = target {
            blocks.push((landing(slot), Label::node(target.as_str())));
        }
    }
    body.insert(
        insert_at,
        PendingInstruction::Instruction(WorkflowInstruction::EndTry {
            try_key: guard_key(&node.id),
        }),
    );
    body.insert(
        0,
        PendingInstruction::BeginTry {
            try_key: guard_key(&node.id),
            catch: node
                .transitions
                .on_failure
                .as_ref()
                .map(|_| landing("on_failure")),
            on_timeout: node
                .transitions
                .on_timeout
                .as_ref()
                .map(|_| landing("on_timeout")),
            on_reject: node
                .transitions
                .on_reject
                .as_ref()
                .map(|_| landing("on_reject")),
            finally: None,
        },
    );
    blocks
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

fn branch_policy(policy: BranchPolicy) -> WorkflowBranchPolicy {
    match policy {
        BranchPolicy::All => WorkflowBranchPolicy::All,
        BranchPolicy::Any => WorkflowBranchPolicy::Any,
        BranchPolicy::FirstSuccess => WorkflowBranchPolicy::FirstSuccess,
    }
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
            key: None,
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
            r#"{"version":1,"instructions":[{"op":"debug_boundary","label":"start"},{"op":"enter_node","node_id":"start"},{"op":"jump","target":3},{"op":"debug_boundary","label":"end"},{"op":"enter_node","node_id":"end"},{"op":"return"}],"source_map":[{"version":1,"instruction_start":0,"instruction_end":3,"node_id":"start","exit_instruction_pointer":2},{"version":1,"instruction_start":3,"instruction_end":6,"node_id":"end"}]}"#
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
            key: None,
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
            module.instructions[2],
            WorkflowInstruction::Jump { target: 3 }
        ));
        assert!(matches!(
            module.instructions[5],
            WorkflowInstruction::Fail { .. }
        ));
        assert_eq!(module.graph_location(5).unwrap().node_id, "fail");
    }

    #[test]
    fn the_source_map_is_ordered_and_covers_every_instruction() {
        // `graph_location` bisects, which is only correct while the ranges stay sorted and
        // disjoint. compile something with branches, a guard, and a loop so the layout is not
        // trivially linear.
        let mut action = node("call", WorkflowNodeKind::Action, Some("end"));
        action.action = Some(WorkflowAction {
            provider: "github".into(),
            function: "deploy".into(),
            timeout_seconds: 30,
            configuration: Default::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: BTreeMap::new(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        });
        action.transitions.on_failure = Some(WorkflowNodeRef::new("recover"));
        action.transitions.on_timeout = Some(WorkflowNodeRef::new("slow"));
        let definition = WorkflowDefinition {
            id: None,
            name: "ordered".into(),
            key: None,
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    node("start", WorkflowNodeKind::Start, Some("call")),
                    action,
                    node("recover", WorkflowNodeKind::End, None),
                    node("slow", WorkflowNodeKind::End, None),
                    node("end", WorkflowNodeKind::End, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };
        let module = compile_workflow_module(&definition).unwrap();

        assert!(
            module.source_map_is_ordered(),
            "source map is out of order or overlapping: {:?}",
            module.source_map
        );
        // and the bisection agrees with a linear scan at every instruction.
        for ip in 0..module.instructions.len() {
            let scanned = module
                .source_map
                .iter()
                .find(|entry| entry.instruction_start <= ip && ip < entry.instruction_end);
            assert_eq!(
                module.graph_location(ip).map(|entry| &entry.node_id),
                scanned.map(|entry| &entry.node_id),
                "bisection disagrees with a scan at instruction {ip}"
            );
        }
    }

    #[test]
    fn failure_edges_compile_into_a_guard_that_routes_by_classification() {
        let mut action = node("call", WorkflowNodeKind::Action, Some("end"));
        action.action = Some(WorkflowAction {
            provider: "github".into(),
            function: "deploy".into(),
            timeout_seconds: 30,
            configuration: Default::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: BTreeMap::new(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        });
        action.transitions.on_failure = Some(WorkflowNodeRef::new("recover"));
        action.transitions.on_timeout = Some(WorkflowNodeRef::new("slow"));
        let definition = WorkflowDefinition {
            id: None,
            name: "edges".into(),
            key: None,
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    node("start", WorkflowNodeKind::Start, Some("call")),
                    action,
                    node("recover", WorkflowNodeKind::End, None),
                    node("slow", WorkflowNodeKind::End, None),
                    node("end", WorkflowNodeKind::End, None),
                ],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };

        let module = compile_workflow_module(&definition).unwrap();
        let Some(WorkflowInstruction::BeginTry {
            try_key,
            catch,
            on_timeout,
            on_reject,
            finally,
        }) = module
            .instructions
            .iter()
            .find(|instruction| matches!(instruction, WorkflowInstruction::BeginTry { .. }))
        else {
            panic!("a node with failure edges must open a guard");
        };
        assert_eq!(try_key, "call#edge");
        assert_eq!(*finally, None);
        assert_eq!(*on_reject, None, "no on_reject edge was authored");

        // each landing block closes the guard before jumping, so a re-entered node opens a fresh
        // one instead of finding the previous visit's.
        let recover = catch.expect("an on_failure edge was authored");
        assert!(matches!(
            module.instructions[recover],
            WorkflowInstruction::EndTry { .. }
        ));
        let WorkflowInstruction::Jump { target } = module.instructions[recover + 1] else {
            panic!("the on_failure landing must jump to its authored target");
        };
        assert_eq!(module.graph_location(target).unwrap().node_id, "recover");

        // the landing block is attributed to its node *and* names the edge that reached it, which
        // is what the cursors endpoint shows an operator.
        let recover_location = module.graph_location(recover).unwrap();
        assert_eq!(recover_location.node_id, "call");
        assert_eq!(recover_location.edge_label.as_deref(), Some("on_failure"));

        let timeout = on_timeout.unwrap();
        assert_eq!(
            module
                .graph_location(timeout)
                .unwrap()
                .edge_label
                .as_deref(),
            Some("on_timeout")
        );
        assert!(matches!(
            module.instructions[timeout],
            WorkflowInstruction::EndTry { .. }
        ));
        let WorkflowInstruction::Jump { target } = module.instructions[timeout + 1] else {
            panic!("the on_timeout landing must jump to its authored target");
        };
        assert_eq!(module.graph_location(target).unwrap().node_id, "slow");

        // and the success path closes the same guard before taking `next`.
        let end_try = module
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, WorkflowInstruction::EndTry { .. }))
            .unwrap();
        assert_eq!(module.graph_location(end_try).unwrap().node_id, "call");
        assert_eq!(
            module.graph_location(end_try).unwrap().edge_label,
            None,
            "a node's own block is not an edge landing"
        );
        let WorkflowInstruction::Jump { target } = module.instructions[end_try + 1] else {
            panic!("the guarded body must jump on to `next`");
        };
        assert_eq!(module.graph_location(target).unwrap().node_id, "end");
    }

    #[test]
    fn declared_interrupt_handlers_compile_into_the_module_and_a_safe_point() {
        let mut action = node("call", WorkflowNodeKind::Action, Some("end"));
        action.action = Some(WorkflowAction {
            provider: "github".into(),
            function: "deploy".into(),
            timeout_seconds: 30,
            configuration: Default::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: BTreeMap::new(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        });
        let mut handler = node("halt", WorkflowNodeKind::Interrupt, Some("give_up"));
        handler.kind = WorkflowNodeKind::Interrupt;
        let mut resume = node("give_up", WorkflowNodeKind::Resume, None);
        resume.parameters = serde_json::from_value(serde_json::json!({ "mode": "fail" })).unwrap();
        let definition = WorkflowDefinition {
            id: None,
            name: "interrupts".into(),
            key: None,
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![
                    node("start", WorkflowNodeKind::Start, Some("call")),
                    action,
                    node("end", WorkflowNodeKind::End, None),
                    handler,
                    resume,
                ],
                metadata: serde_json::from_value(serde_json::json!({
                    "interrupts": [{ "on": "external", "handler": "halt" }]
                }))
                .unwrap(),
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };

        let module = compile_workflow_module(&definition).unwrap();
        // the handler table is frozen into the module, so a run keeps the handlers it started with
        // even after the definition is edited.
        assert_eq!(module.interrupt_handlers.len(), 1);
        let compiled = &module.interrupt_handlers[0];
        assert_eq!(compiled.source, InterruptSource::External);
        assert_eq!(
            module.graph_location(compiled.target).unwrap().node_id,
            "halt"
        );

        // an interruptible node gets a safe point; `start` and the terminals do not.
        let checks: Vec<&str> = module
            .instructions
            .iter()
            .enumerate()
            .filter(|(_, instruction)| {
                matches!(instruction, WorkflowInstruction::CheckInterrupt { .. })
            })
            .map(|(ip, _)| module.graph_location(ip).unwrap().node_id.as_str())
            .collect();
        assert_eq!(checks, vec!["call"]);

        // and the region's `resume` is a real opcode, not a no-op pure node.
        assert!(module.instructions.iter().any(|instruction| matches!(
            instruction,
            WorkflowInstruction::ResumeInterrupt {
                mode: runinator_models::interrupt::InterruptMode::Fail
            }
        )));
    }

    #[test]
    fn coordination_effect_carries_parameters_without_graph_node_references() {
        let mut mutex = node("mutex", WorkflowNodeKind::Mutex, Some("end"));
        mutex.parameters =
            serde_json::from_value(serde_json::json!({ "name": "sdlc-development" })).unwrap();
        mutex.timeout_seconds = Some(600);

        let mut instructions = Vec::new();
        lower_node(&mutex, &mut instructions).unwrap();

        let Some(PendingInstruction::Instruction(WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Coordination { kind, input },
        })) = instructions.first()
        else {
            panic!("mutex must lower to a coordination effect");
        };
        assert_eq!(kind, "mutex");
        assert_eq!(
            input,
            &runinator_models::json!({ "name": "sdlc-development", "timeout_seconds": 600 })
        );
        assert!(input.get("transitions").is_none());
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
            workspace_affinity: None,
            execution_profile: None,
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
            key: None,
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
                            workspace_affinity,
                            idempotency_key,
                            function_binding,
                            ..
                        },
                } => Some((
                    provider,
                    function,
                    input,
                    timeout_seconds,
                    retry,
                    tags,
                    required_labels,
                    workspace_affinity,
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
        assert_eq!(request.7, &None);
        assert_eq!(request.8, &Some(Value::String("order-42".into())));
        assert_eq!(
            request
                .9
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
            key: None,
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
            workspace_affinity: None,
            execution_profile: None,
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
            key: None,
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

    #[test]
    fn structured_control_nodes_lower_to_frames_not_selector_payloads() {
        let mut loop_node = node("loop", WorkflowNodeKind::Loop, Some("body"));
        loop_node.parameters =
            serde_json::from_value(serde_json::json!({"items": [1, 2]})).unwrap();
        loop_node.transitions.on_success = Some(WorkflowNodeRef::new("done"));
        loop_node.max_iterations = Some(5);
        let mut instructions = Vec::new();
        lower_node(&loop_node, &mut instructions).unwrap();
        assert!(matches!(instructions.as_slice(), [
            PendingInstruction::Instruction(WorkflowInstruction::Evaluate { .. }),
            PendingInstruction::BeginLoop { loop_key, max_iterations: Some(5), .. },
        ] if loop_key == "loop"));

        let mut try_node = node("guard", WorkflowNodeKind::Try, Some("done"));
        try_node.parameters = serde_json::from_value(serde_json::json!({
            "body": {"$node": "body"}, "catch": {"$node": "catch"}, "finally": {"$node": "finally"}
        }))
        .unwrap();
        let mut instructions = Vec::new();
        lower_node(&try_node, &mut instructions).unwrap();
        assert!(
            matches!(instructions.first(), Some(PendingInstruction::BeginTry { try_key, .. }) if try_key == "guard")
        );
        assert!(matches!(
            instructions.get(1),
            Some(PendingInstruction::Jump(_))
        ));
        assert!(
            !instructions
                .iter()
                .any(|instruction| matches!(instruction, PendingInstruction::Select(..)))
        );

        let mut action_node = node("charge", WorkflowNodeKind::Action, None);
        action_node.action = Some(WorkflowAction {
            provider: "billing".into(),
            function: "charge".into(),
            timeout_seconds: 10,
            configuration: Default::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: BTreeMap::new(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        });
        action_node.compensation = Some(WorkflowAction {
            provider: "billing".into(),
            function: "refund".into(),
            timeout_seconds: 10,
            configuration: Default::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: BTreeMap::new(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        });
        let mut instructions = Vec::new();
        lower_node(&action_node, &mut instructions).unwrap();
        assert!(
            matches!(instructions.get(1), Some(PendingInstruction::Instruction(
            WorkflowInstruction::RegisterCompensation { compensation_key, .. }
        )) if compensation_key == "charge")
        );
    }

    #[test]
    fn reentry_guard_targets_the_first_real_node_instruction() {
        let mut start = node("start", WorkflowNodeKind::Start, Some("end"));
        start.reentry.enabled = true;
        start.reentry.max_visits = 2;
        let definition = WorkflowDefinition {
            id: None,
            name: "reentry".into(),
            key: None,
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: WorkflowGraph {
                start: Some("start".into()),
                nodes: vec![start, node("end", WorkflowNodeKind::End, None)],
                ..Default::default()
            },
            created_at: None,
            updated_at: None,
        };
        let module = compile_workflow_module(&definition).unwrap();
        assert!(matches!(
            module.instructions.get(2),
            Some(WorkflowInstruction::Reenter {
                target: 3,
                max_visits: 2,
                ..
            })
        ));
    }

    #[test]
    fn concurrency_nodes_lower_to_explicit_vm_coordination_instructions() {
        let mut parallel = node("parallel", WorkflowNodeKind::Parallel, None);
        parallel.parameters =
            runinator_models::workflows::WorkflowObject::from_value(runinator_models::json!({
                "branches": [{ "$node": "left" }, { "$node": "right" }]
            }))
            .unwrap();
        let mut join = node("join", WorkflowNodeKind::Join, None);
        join.parameters =
            runinator_models::workflows::WorkflowObject::from_value(runinator_models::json!({
                "wait_for": [{ "$node": "left" }, { "$node": "right" }], "mode": "any"
            }))
            .unwrap();
        let mut race = node("race", WorkflowNodeKind::Race, None);
        race.parameters =
            runinator_models::workflows::WorkflowObject::from_value(runinator_models::json!({
                "branches": [{ "$node": "left" }, { "$node": "right" }], "winner": "all"
            }))
            .unwrap();
        let mut map = node("map", WorkflowNodeKind::Map, Some("done"));
        map.parameters =
            runinator_models::workflows::WorkflowObject::from_value(runinator_models::json!({
                "items": [1, 2], "target": { "$node": "work" }, "concurrency": 2
            }))
            .unwrap();

        let mut instructions = Vec::new();
        lower_node(&parallel, &mut instructions).unwrap();
        assert!(
            matches!(instructions.as_slice(), [PendingInstruction::Fork(_, key)] if key == "parallel")
        );
        instructions.clear();
        lower_node(&join, &mut instructions).unwrap();
        assert!(matches!(
            instructions.as_slice(),
            [PendingInstruction::Instruction(WorkflowInstruction::Join {
                expected: 2,
                mode: WorkflowBranchPolicy::Any,
                ..
            })]
        ));
        instructions.clear();
        lower_node(&race, &mut instructions).unwrap();
        assert!(matches!(
            instructions.as_slice(),
            [PendingInstruction::Race {
                winner: WorkflowBranchPolicy::All,
                ..
            }]
        ));
        instructions.clear();
        lower_node(&map, &mut instructions).unwrap();
        assert!(matches!(
            instructions.get(1),
            Some(PendingInstruction::BeginMap { concurrency: 2, .. })
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
