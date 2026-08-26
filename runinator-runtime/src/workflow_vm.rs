//! Host-free interpreter for [`runinator_models::workflow_vm::WorkflowModule`].
//!
//! The machine stops at durable boundaries. Its caller is responsible for assigning effect ids and
//! atomically persisting the returned continuation and effect receipt.

use runinator_models::{
    interrupt::{InterruptMode, InterruptSource},
    value::Value,
    workflow_vm::{
        WorkflowCompensationFrame, WorkflowContinuation, WorkflowContinuationStatus,
        WorkflowEffectRequest, WorkflowFailure, WorkflowFailureKind, WorkflowForkFrame,
        WorkflowFrame, WorkflowInstruction, WorkflowInterruptFrame, WorkflowInterruptOutcome,
        WorkflowLoopFrame, WorkflowMapFrame, WorkflowModule, WorkflowRaceFrame, WorkflowTryFrame,
        WorkflowTryPhase,
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
    /// A thread reached a safe point with an interrupt to service. The host persists both records
    /// in one transaction: the frozen thread, and the handler continuation now running beside it.
    Interrupted {
        suspended: WorkflowContinuation,
        handler: WorkflowContinuation,
        source: runinator_models::interrupt::InterruptSource,
    },
    /// A handler finished. The host retires it and applies `outcome` to the thread it suspended.
    InterruptResolved {
        handler: WorkflowContinuation,
        interrupted_continuation_id: uuid::Uuid,
        outcome: WorkflowInterruptOutcome,
    },
}

/// Resume a continuation after the host durably settled its sole outstanding effect.
///
/// `request` is the settled effect's own request. It is what lets the VM classify the arrival —
/// a timer elapsing, a park being resolved, a child run finishing — which is how the drive-matched
/// interrupt sources are detected without a host re-reading graph ancestry.
pub fn resume(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    request: Option<&WorkflowEffectRequest>,
    result: Result<Value, WorkflowFailure>,
) -> WorkflowVmStep {
    // Persisted effect settlement makes the row runnable so a scheduler can claim it, while the
    // effect id remains available for the durable host to load its immutable receipt.
    if !matches!(
        continuation.status,
        WorkflowContinuationStatus::Waiting | WorkflowContinuationStatus::Runnable
    ) || continuation.awaiting_effect_id.is_none()
    {
        return fail(
            continuation,
            "attempted to resume a continuation that is not waiting for an effect".into(),
        );
    }
    continuation.awaiting_effect_id = None;
    continuation.status = WorkflowContinuationStatus::Runnable;
    // Compensation is best-effort: both a successful and a failed undo settle the active entry,
    // then the next undo is issued. The original failure remains the terminal outcome once the
    // LIFO stack drains, regardless of which graph block happens to follow the failing node in
    // bytecode layout.
    if let Some(position) = continuation.frames.iter().position(
        |frame| matches!(frame, WorkflowFrame::Compensation(frame) if frame.active.is_some()),
    ) {
        let frame = match &mut continuation.frames[position] {
            WorkflowFrame::Compensation(frame) => frame,
            _ => unreachable!("compensation frame position was checked"),
        };
        frame.active = None;
        if let Some(request) = frame.pending.pop() {
            frame.active = Some(request.clone());
            return yield_effect(continuation, request);
        }
        continuation.frames.remove(position);
        if let Some(Value::String(message)) = continuation
            .locals
            .remove("__workflow_vm_compensation_failure")
        {
            return fail(continuation, message);
        }
    }
    // an arriving result is a safe point for the sources the VM can classify for itself. a handler
    // continuation is excluded: it may not be interrupted, and it may not fail the run.
    if let Some(source) = arrival_interrupt_source(request, &result)
        && let Some(step) = try_raise_detected(module, &continuation, source)
    {
        return step;
    }
    match result {
        Ok(value) => continuation.stack.push(value),
        Err(failure) => return handle_classified_failure(module, continuation, failure),
    }
    step(module, continuation)
}

/// The interrupt source a settled effect represents, in [`InterruptSource::ALL`] precedence.
///
/// `Retry` is the one source with no counterpart here: a re-dispatch is the effect host's business
/// and never reaches the VM as a step, so nothing in a continuation can observe it.
fn arrival_interrupt_source(
    request: Option<&WorkflowEffectRequest>,
    result: &Result<Value, WorkflowFailure>,
) -> Option<InterruptSource> {
    if let Err(failure) = result {
        return match failure.kind {
            WorkflowFailureKind::TimedOut => Some(InterruptSource::Timeout),
            WorkflowFailureKind::Failed => Some(InterruptSource::Failure),
            // a cancel is not a condition a handler gets to reconsider.
            WorkflowFailureKind::Canceled | WorkflowFailureKind::Rejected => None,
        };
    }
    match request? {
        WorkflowEffectRequest::Timer { .. } | WorkflowEffectRequest::TimerDelay { .. } => {
            Some(InterruptSource::Wake)
        }
        WorkflowEffectRequest::ChildRun { .. } => Some(InterruptSource::Child),
        WorkflowEffectRequest::Signal { .. }
        | WorkflowEffectRequest::Approval { .. }
        | WorkflowEffectRequest::Input { .. } => Some(InterruptSource::Resolved),
        _ => None,
    }
}

/// Raise a VM-detected interrupt, if everything the fail-open rules ask for holds.
fn try_raise_detected(
    module: &WorkflowModule,
    continuation: &WorkflowContinuation,
    source: InterruptSource,
) -> Option<WorkflowVmStep> {
    if interrupt_frame(continuation).is_some() {
        return None;
    }
    let handler = module.interrupt_handler(source)?;
    let location = module.graph_location(continuation.instruction_pointer)?;
    if !location.interruptible {
        return None;
    }
    Some(raise_interrupt(
        module,
        continuation.clone(),
        source,
        Value::Null,
        handler.target,
    ))
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
            WorkflowInstruction::EnterNode { node_id } => {
                continuation.pending_node_entries.push(node_id.clone());
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
            WorkflowInstruction::Evaluate {
                module: invocation_module,
            } => {
                let context = Value::Object(
                    continuation
                        .locals
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                );
                match runinator_compute::evaluate_module_pure(
                    invocation_module,
                    &context,
                    &runinator_compute::CallableCatalog::builtin(),
                ) {
                    Ok(value) => {
                        continuation.stack.push(value);
                        continuation.instruction_pointer += 1;
                    }
                    Err(error) => return handle_failure(module, continuation, error.to_string()),
                }
            }
            WorkflowInstruction::Select {
                kind,
                configuration,
                targets,
                default,
            } => match select_target(kind, configuration, targets, *default, &continuation) {
                Ok(Some(target)) => continuation.instruction_pointer = target,
                Ok(None) => {
                    return handle_failure(
                        module,
                        continuation,
                        "selector has no matching target".into(),
                    );
                }
                Err(message) => return handle_failure(module, continuation, message),
            },
            // PureNode remains a compatibility opcode for modules compiled before dedicated
            // instructions existed.  The only such graph operation is Resume; it becomes a
            // no-op outside an interrupt handler and is otherwise handled by ResumeInterrupt.
            WorkflowInstruction::PureNode { .. } => continuation.instruction_pointer += 1,
            WorkflowInstruction::NextLoop { loop_key } => {
                let Some(position) = continuation.frames.iter().rposition(|frame| {
                    matches!(frame, WorkflowFrame::Loop(frame) if frame.loop_key == *loop_key)
                }) else {
                    return handle_failure(module, continuation, format!("next_loop '{loop_key}' has no frame"));
                };
                let mut frame = match continuation.frames.remove(position) {
                    WorkflowFrame::Loop(frame) => frame,
                    _ => unreachable!(),
                };
                if let Some(value) = continuation.stack.pop() {
                    frame.results.push(value);
                }
                frame.index += 1;
                if frame.index < frame.items.len() as u64
                    && frame
                        .max_iterations
                        .map(|limit| frame.index < limit)
                        .unwrap_or(true)
                {
                    continuation.locals.insert(
                        format!("{loop_key}.item"),
                        frame.items[frame.index as usize].clone(),
                    );
                    continuation
                        .locals
                        .insert(format!("{loop_key}.index"), Value::from(frame.index as i64));
                    continuation.instruction_pointer = frame.body;
                    continuation.frames.push(WorkflowFrame::Loop(frame));
                } else {
                    continuation.stack.push(Value::Array(frame.results));
                    continuation.instruction_pointer = frame.exit;
                }
            }
            // The safe point an externally requested interrupt fires at. Everything about the
            // decision is on the continuation and the frozen module, so it stays a pure step.
            WorkflowInstruction::CheckInterrupt { handlers } => {
                let Some(pending) = continuation.pending_interrupt.take() else {
                    continuation.instruction_pointer += 1;
                    continue;
                };
                match handlers
                    .iter()
                    .find(|handler| handler.source == pending.source)
                {
                    // fail-open, and the request is consumed either way: a source nobody declared
                    // a handler for must not linger and fire at some arbitrary later point.
                    None => continuation.instruction_pointer += 1,
                    Some(handler) => {
                        return raise_interrupt(
                            module,
                            continuation,
                            pending.source,
                            pending.payload,
                            handler.target,
                        );
                    }
                }
            }
            WorkflowInstruction::ResumeInterrupt { mode } => {
                return resolve_interrupt(module, continuation, *mode);
            }
            WorkflowInstruction::DebugBoundary { label } => {
                let mut park_after_boundary = false;
                if let Some(position) = continuation
                    .frames
                    .iter()
                    .rposition(|frame| matches!(frame, WorkflowFrame::Debug(_)))
                {
                    if let WorkflowFrame::Debug(frame) = &mut continuation.frames[position] {
                        frame.breakpoint = label.clone();
                        if frame.step_requested {
                            frame.step_requested = false;
                            frame.paused = true;
                            park_after_boundary = true;
                        }
                    }
                } else {
                    continuation.frames.push(WorkflowFrame::Debug(
                        runinator_models::workflow_vm::WorkflowDebugFrame {
                            paused: false,
                            step_requested: false,
                            breakpoint: label.clone(),
                            last_output: None,
                            speculative: false,
                        },
                    ));
                }
                continuation.instruction_pointer += 1;
                if park_after_boundary {
                    continuation.status = WorkflowContinuationStatus::Paused;
                    continuation.operator_paused = true;
                    return WorkflowVmStep::Joined {
                        continuation,
                        join_key: "debug-step".into(),
                        value: Value::Null,
                    };
                }
            }
            WorkflowInstruction::SetOutput {
                event_type,
                artifacts,
            } => {
                let context = local_context(&continuation);
                let mut artifact_values = runinator_models::value::Map::new();
                for artifact in artifacts {
                    let value = match runinator_compute::evaluate_module_pure(
                        &artifact.source,
                        &context,
                        &runinator_compute::CallableCatalog::builtin(),
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            return handle_failure(module, continuation, error.to_string());
                        }
                    };
                    artifact_values.insert(artifact.name.clone(), value);
                }
                let data = continuation.stack.last().cloned().unwrap_or(Value::Null);
                let mut output = runinator_models::value::Map::new();
                output.insert("data".into(), data);
                output.insert("artifacts".into(), Value::Object(artifact_values));
                if let Some(event_type) = event_type {
                    output.insert("event_type".into(), Value::String(event_type.clone()));
                }
                continuation
                    .locals
                    .insert("__workflow_vm_output".into(), Value::Object(output));
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::BeginLoop {
                loop_key,
                body,
                exit,
                max_iterations,
            } => {
                let existing = continuation.frames.iter().position(|frame| {
                    matches!(frame, WorkflowFrame::Loop(frame) if frame.loop_key == *loop_key)
                });
                let frame = if let Some(position) = existing {
                    // The compiler evaluates `items` on every graph re-entry. Discard that
                    // stable expression result, then retain the body result beneath it.
                    let _items = continuation.stack.pop();
                    let mut frame = match continuation.frames.remove(position) {
                        WorkflowFrame::Loop(frame) => frame,
                        _ => unreachable!("loop frame position was checked"),
                    };
                    if let Some(result) = continuation.stack.pop() {
                        frame.results.push(result);
                    }
                    frame.index += 1;
                    frame
                } else {
                    let Some(Value::Array(items)) = continuation.stack.pop() else {
                        return handle_failure(
                            module,
                            continuation,
                            format!("loop '{loop_key}' needs an array value"),
                        );
                    };
                    WorkflowLoopFrame {
                        loop_key: loop_key.clone(),
                        body: *body,
                        exit: *exit,
                        index: 0,
                        items,
                        results: Vec::new(),
                        max_iterations: *max_iterations,
                    }
                };
                let has_next = frame.index < frame.items.len() as u64
                    && frame
                        .max_iterations
                        .map(|limit| frame.index < limit)
                        .unwrap_or(true);
                if has_next {
                    continuation.locals.insert(
                        format!("{loop_key}.item"),
                        frame.items[frame.index as usize].clone(),
                    );
                    continuation
                        .locals
                        .insert(format!("{loop_key}.index"), Value::from(frame.index as i64));
                    continuation.frames.push(WorkflowFrame::Loop(frame.clone()));
                    continuation.instruction_pointer = frame.body;
                } else {
                    continuation.stack.push(Value::Array(frame.results));
                    continuation.instruction_pointer = frame.exit;
                }
            }
            WorkflowInstruction::Reenter {
                reentry_key,
                target,
                exhausted,
                max_visits,
            } => {
                let position = continuation.frames.iter().position(|frame| {
                    matches!(frame, WorkflowFrame::Reentry(frame) if frame.reentry_key == *reentry_key)
                });
                let visits = match position {
                    Some(position) => match &mut continuation.frames[position] {
                        WorkflowFrame::Reentry(frame) => {
                            frame.visits += 1;
                            frame.visits
                        }
                        _ => unreachable!("reentry frame position was checked"),
                    },
                    None => {
                        continuation.frames.push(WorkflowFrame::Reentry(
                            runinator_models::workflow_vm::WorkflowReentryFrame {
                                reentry_key: reentry_key.clone(),
                                visits: 1,
                                max_visits: *max_visits,
                            },
                        ));
                        1
                    }
                };
                if visits > *max_visits {
                    match exhausted {
                        Some(target) => continuation.instruction_pointer = *target,
                        None => {
                            return handle_failure(
                                module,
                                continuation,
                                format!(
                                    "reentry '{reentry_key}' exhausted after {max_visits} visits"
                                ),
                            );
                        }
                    }
                } else {
                    continuation.instruction_pointer = *target;
                }
            }
            WorkflowInstruction::BeginTry {
                try_key,
                catch,
                on_timeout,
                on_reject,
                finally,
            } => {
                let position = continuation.frames.iter().rposition(
                    |frame| matches!(frame, WorkflowFrame::Try(frame) if frame.try_key == *try_key),
                );
                if let Some(position) = position {
                    let frame = match continuation.frames.remove(position) {
                        WorkflowFrame::Try(frame) => frame,
                        _ => unreachable!("try frame position was checked"),
                    };
                    match frame.phase {
                        WorkflowTryPhase::Body | WorkflowTryPhase::Catch
                            if frame.finally.is_some() =>
                        {
                            let finally = frame.finally.expect("checked");
                            continuation
                                .frames
                                .push(WorkflowFrame::Try(WorkflowTryFrame {
                                    phase: WorkflowTryPhase::Finally,
                                    ..frame
                                }));
                            continuation.instruction_pointer = finally;
                        }
                        WorkflowTryPhase::Finally if frame.pending_failure.is_some() => {
                            return handle_failure(
                                module,
                                continuation,
                                frame.pending_failure.expect("checked"),
                            );
                        }
                        _ => continuation.instruction_pointer += 2,
                    }
                } else {
                    continuation
                        .frames
                        .push(WorkflowFrame::Try(WorkflowTryFrame {
                            try_key: try_key.clone(),
                            phase: WorkflowTryPhase::Body,
                            catch: *catch,
                            on_timeout: *on_timeout,
                            on_reject: *on_reject,
                            finally: *finally,
                            pending_failure: None,
                        }));
                    continuation.instruction_pointer += 1;
                }
            }
            WorkflowInstruction::EndTry { try_key } => {
                let Some(position) = continuation.frames.iter().rposition(
                    |frame| matches!(frame, WorkflowFrame::Try(frame) if frame.try_key == *try_key),
                ) else {
                    return handle_failure(
                        module,
                        continuation,
                        format!("end_try '{try_key}' has no frame"),
                    );
                };
                let frame = match continuation.frames.remove(position) {
                    WorkflowFrame::Try(frame) => frame,
                    _ => unreachable!(),
                };
                if let Some(finally) = frame.finally {
                    continuation
                        .frames
                        .push(WorkflowFrame::Try(WorkflowTryFrame {
                            phase: WorkflowTryPhase::Finally,
                            ..frame
                        }));
                    continuation.instruction_pointer = finally;
                } else if let Some(message) = frame.pending_failure {
                    return handle_failure(module, continuation, message);
                } else {
                    continuation.instruction_pointer += 1;
                }
            }
            WorkflowInstruction::RegisterCompensation {
                compensation_key: _,
                request,
            } => {
                let position = continuation
                    .frames
                    .iter()
                    .position(|frame| matches!(frame, WorkflowFrame::Compensation(_)));
                if let Some(position) = position {
                    if let WorkflowFrame::Compensation(frame) = &mut continuation.frames[position] {
                        frame.pending.push(request.clone());
                    }
                } else {
                    continuation.frames.push(WorkflowFrame::Compensation(
                        WorkflowCompensationFrame {
                            pending: vec![request.clone()],
                            active: None,
                            resume: None,
                        },
                    ));
                }
                continuation.instruction_pointer += 1;
            }
            WorkflowInstruction::BeginCompensation { resume } => {
                let Some(position) = continuation
                    .frames
                    .iter()
                    .position(|frame| matches!(frame, WorkflowFrame::Compensation(_)))
                else {
                    continuation.instruction_pointer =
                        resume.unwrap_or(continuation.instruction_pointer + 1);
                    continue;
                };
                let frame = match &mut continuation.frames[position] {
                    WorkflowFrame::Compensation(frame) => frame,
                    _ => unreachable!(),
                };
                frame.active = None;
                frame.resume = *resume;
                if let Some(request) = frame.pending.pop() {
                    frame.active = Some(request.clone());
                    return yield_effect(continuation, request);
                }
                let resume = frame.resume.unwrap_or(continuation.instruction_pointer + 1);
                continuation.frames.remove(position);
                let message = continuation
                    .locals
                    .remove("__workflow_vm_compensation_failure");
                if let Some(Value::String(message)) = message {
                    return fail(continuation, message);
                }
                continuation.instruction_pointer = resume;
            }
            WorkflowInstruction::Effect { request } => {
                return yield_effect(continuation, request.clone());
            }
            WorkflowInstruction::Fork { targets, join_key } => {
                return fork(continuation, targets, join_key, None);
            }
            WorkflowInstruction::Race {
                targets,
                race_key,
                winner,
            } => {
                if targets.is_empty() {
                    return fail(continuation, "race needs at least one target".into());
                }
                continuation
                    .frames
                    .push(WorkflowFrame::Race(WorkflowRaceFrame {
                        race_key: race_key.clone(),
                        expected: targets.len() as u64,
                        winner_policy: *winner,
                        winner: None,
                        winner_value: None,
                    }));
                let WorkflowVmStep::Fork {
                    parent,
                    mut children,
                    join_key,
                } = fork(continuation, targets, race_key, Some("race"))
                else {
                    unreachable!("non-empty race targets always fork")
                };
                // The race coordinator belongs only to the parked parent. A contender retains
                // its fork provenance, but cannot independently nominate or overwrite a winner.
                for child in &mut children {
                    child
                        .frames
                        .retain(|frame| !matches!(frame, WorkflowFrame::Race(_)));
                }
                return WorkflowVmStep::Fork {
                    parent,
                    children,
                    join_key,
                };
            }
            WorkflowInstruction::BeginMap {
                map_key,
                body,
                exit,
                concurrency,
            } => {
                if *concurrency == 0 {
                    return fail(
                        continuation,
                        "map concurrency must be greater than zero".into(),
                    );
                }
                if continuation.frames.iter().any(|frame| {
                    matches!(frame, WorkflowFrame::Map(frame) if frame.map_key == *map_key && frame.item_index.is_some())
                }) {
                    // A map body returns to its map node through the normal graph edge. The
                    // compiler evaluates `items` again on that entry; discard that stable
                    // expression value and report the body's result beneath it.
                    let _items = continuation.stack.pop();
                    let value = continuation.stack.pop().unwrap_or(Value::Null);
                    continuation.status = WorkflowContinuationStatus::Joined;
                    return WorkflowVmStep::Joined {
                        continuation,
                        join_key: map_key.clone(),
                        value,
                    };
                }
                let Some(Value::Array(items)) = continuation.stack.pop() else {
                    return fail(
                        continuation,
                        format!("map '{map_key}' needs an array value"),
                    );
                };
                if items.is_empty() {
                    continuation.stack.push(Value::Array(Vec::new()));
                    continuation.instruction_pointer = *exit;
                    continue;
                }
                let count = (*concurrency as usize).min(items.len());
                continuation
                    .frames
                    .push(WorkflowFrame::Map(WorkflowMapFrame {
                        map_key: map_key.clone(),
                        body: *body,
                        exit: *exit,
                        concurrency: *concurrency,
                        next_index: count as u64,
                        items: items.clone(),
                        results: Vec::new(),
                        item: None,
                        item_index: None,
                    }));
                let targets = std::iter::repeat_n(*body, count).collect::<Vec<_>>();
                return fork_map(continuation, &targets, map_key, &items[..count]);
            }
            WorkflowInstruction::Join {
                join_key,
                expected,
                mode,
            } => {
                let value = continuation.stack.pop().unwrap_or(Value::Null);
                continuation.frames.push(WorkflowFrame::Join(
                    runinator_models::workflow_vm::WorkflowJoinFrame {
                        join_key: join_key.clone(),
                        expected: *expected,
                        mode: *mode,
                        arrivals: Vec::new(),
                    },
                ));
                continuation.status = WorkflowContinuationStatus::Joined;
                return WorkflowVmStep::Joined {
                    continuation,
                    join_key: join_key.clone(),
                    value,
                };
            }
            WorkflowInstruction::Return => {
                // a handler region that runs off its end without an explicit `resume` still has to
                // hand control back, or the thread it froze would never move again.
                if interrupt_frame(&continuation).is_some() {
                    return resolve_interrupt(module, continuation, InterruptMode::Resume);
                }
                let value = continuation.stack.pop().unwrap_or(Value::Null);
                continuation.status = WorkflowContinuationStatus::Succeeded;
                return WorkflowVmStep::Complete {
                    continuation,
                    value,
                };
            }
            WorkflowInstruction::Fail { message } => {
                return handle_failure(module, continuation, message.clone());
            }
        }
    }
    fail(continuation, "workflow instruction budget exhausted".into())
}

fn fork(
    mut parent: WorkflowContinuation,
    targets: &[usize],
    key: &str,
    kind: Option<&str>,
) -> WorkflowVmStep {
    if targets.is_empty() {
        return fail(parent, "fork needs at least one target".into());
    }
    let mut children = Vec::with_capacity(targets.len());
    for (branch, target) in targets.iter().enumerate() {
        let mut child = parent.clone();
        child.id = stable_id(
            parent.id,
            &format!("{}:{key}:{branch}", kind.unwrap_or("fork")),
        );
        child.parent_id = Some(parent.id);
        child.fork_key = Some(key.to_owned());
        // Entries before the fork belong to the parent branch. Each child will collect only the
        // nodes it enters after its own target, preventing duplicated history at fan-out.
        child.pending_node_entries.clear();
        child.instruction_pointer = *target;
        child.awaiting_effect_id = None;
        child.status = WorkflowContinuationStatus::Runnable;
        child.frames.push(WorkflowFrame::Fork(WorkflowForkFrame {
            fork_key: key.to_owned(),
            parent_id: parent.id,
            branch_index: branch as u64,
        }));
        children.push(child);
    }
    parent.instruction_pointer += 1;
    parent.status = WorkflowContinuationStatus::Joined;
    WorkflowVmStep::Fork {
        parent,
        children,
        join_key: key.to_owned(),
    }
}

fn fork_map(
    parent: WorkflowContinuation,
    targets: &[usize],
    map_key: &str,
    items: &[Value],
) -> WorkflowVmStep {
    let WorkflowVmStep::Fork {
        parent,
        mut children,
        join_key,
    } = fork(parent, targets, map_key, Some("map"))
    else {
        unreachable!("non-empty map targets always fork")
    };
    for (index, child) in children.iter_mut().enumerate() {
        child.frames.retain(|frame| {
            !matches!(frame, WorkflowFrame::Map(frame) if frame.map_key == map_key && frame.item_index.is_none())
        });
        child
            .locals
            .insert(format!("{map_key}.item"), items[index].clone());
        child
            .locals
            .insert(format!("{map_key}.index"), Value::from(index as i64));
        child.frames.push(WorkflowFrame::Map(WorkflowMapFrame {
            map_key: map_key.to_owned(),
            body: child.instruction_pointer,
            exit: 0,
            concurrency: 0,
            next_index: 0,
            items: Vec::new(),
            results: Vec::new(),
            item: Some(items[index].clone()),
            item_index: Some(index as u64),
        }));
    }
    WorkflowVmStep::Fork {
        parent,
        children,
        join_key,
    }
}

fn fail(mut continuation: WorkflowContinuation, message: String) -> WorkflowVmStep {
    // a handler cannot fail the run. the strongest thing it can say is `resume fail`, and a handler
    // that breaks on its own gives the interrupted thread back untouched instead.
    if let Some(frame) = interrupt_frame(&continuation) {
        let interrupted_continuation_id = frame.interrupted_continuation_id;
        let instruction_pointer = frame.resume_instruction_pointer;
        continuation.status = WorkflowContinuationStatus::Failed;
        return WorkflowVmStep::InterruptResolved {
            handler: continuation,
            interrupted_continuation_id,
            outcome: WorkflowInterruptOutcome::Resume {
                instruction_pointer,
            },
        };
    }
    continuation.status = WorkflowContinuationStatus::Failed;
    WorkflowVmStep::Failed {
        continuation,
        message,
    }
}

fn yield_effect(
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
        request,
    }
}

/// Freeze every context-dependent value before the request crosses the durable effect boundary.
/// A resumed delivery must never re-read mutable workflow inputs or carry authoring `$ref` objects
/// to a provider/infrastructure host.
fn resolve_effect_request(
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
            idempotency_key,
            function_binding,
        } => WorkflowEffectRequest::Action {
            provider,
            function,
            input: resolve(input)?,
            timeout_seconds,
            retry,
            tags,
            required_labels,
            idempotency_key: idempotency_key.map(resolve).transpose()?,
            function_binding,
        },
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

/// Freeze `continuation` and start a handler continuation beside it.
///
/// The handler is a separate continuation rather than a frame on this one, because the interrupted
/// thread must stay exactly where it was: everything the handler does is invisible to it, and the
/// only channel back is the decision its `resume` carries.
/// Build the handler continuation for `source` beside `continuation`, deciding nothing about the
/// interrupted thread itself.
///
/// [`raise_interrupt`] pairs this with a suspend. The engine's retry path calls it directly instead:
/// a thread parked on an effect is already stopped, and suspending it would stop the retried
/// effect's own settlement from resuming it.
pub fn interrupt_handler_continuation(
    module: &WorkflowModule,
    continuation: &WorkflowContinuation,
    source: InterruptSource,
    payload: Value,
    target: usize,
    discriminator: &str,
) -> WorkflowContinuation {
    let location = module.graph_location(continuation.instruction_pointer);
    let frame = WorkflowInterruptFrame {
        source,
        interrupted_continuation_id: continuation.id,
        // the interrupted thread resumes *after* the point it was frozen at, so servicing an
        // interrupt at a safe point cannot re-raise the same one on the next drive.
        resume_instruction_pointer: continuation.instruction_pointer + 1,
        node_start_instruction_pointer: location
            .map(|entry| entry.instruction_start)
            .unwrap_or(continuation.instruction_pointer),
        node_exit_instruction_pointer: location.and_then(|entry| entry.exit_instruction_pointer),
        payload: payload.clone(),
        handled_at_instruction_pointers: vec![continuation.instruction_pointer],
    };

    let mut handler = WorkflowContinuation::start(continuation.workflow_run_id, module.version);
    // the handler id is derived from the interrupted thread, the source, and the caller's
    // discriminator, so a redelivered drive re-raising the same interrupt inserts nothing while a
    // genuinely new occurrence (the next retry attempt) gets its own handler.
    handler.id = stable_id(
        continuation.id,
        &format!("interrupt:{}:{target}:{discriminator}", source.as_str()),
    );
    handler.instruction_pointer = target;
    // the region reads the run's context plus what raised it; it writes nothing back except its
    // `resume` decision.
    handler.locals = continuation.locals.clone();
    handler.locals.insert(
        "interrupt".into(),
        runinator_models::json!({ "source": source.as_str(), "payload": payload }),
    );
    handler.parent_id = Some(continuation.id);
    handler.frames = vec![WorkflowFrame::Interrupt(frame)];
    handler
}

fn raise_interrupt(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    source: InterruptSource,
    payload: Value,
    target: usize,
) -> WorkflowVmStep {
    let handler =
        interrupt_handler_continuation(module, &continuation, source, payload, target, "");
    continuation.status = WorkflowContinuationStatus::Suspended;
    WorkflowVmStep::Interrupted {
        suspended: continuation,
        handler,
        source,
    }
}

/// Finish a handler and say what the thread it suspended should do next.
fn resolve_interrupt(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    mode: InterruptMode,
) -> WorkflowVmStep {
    let Some(position) = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Interrupt(_)))
    else {
        return handle_failure(
            module,
            continuation,
            "resume_interrupt has no interrupt frame".into(),
        );
    };
    let frame = match continuation.frames.remove(position) {
        WorkflowFrame::Interrupt(frame) => frame,
        _ => unreachable!("interrupt frame position was checked"),
    };
    let outcome = match mode {
        InterruptMode::Resume => WorkflowInterruptOutcome::Resume {
            instruction_pointer: frame.resume_instruction_pointer,
        },
        InterruptMode::Restart => WorkflowInterruptOutcome::Resume {
            instruction_pointer: frame.node_start_instruction_pointer,
        },
        // a node with no single exit cannot be stepped past, so `continue` degrades to `resume`
        // rather than guessing a location.
        InterruptMode::Continue => WorkflowInterruptOutcome::Resume {
            instruction_pointer: frame
                .node_exit_instruction_pointer
                .unwrap_or(frame.resume_instruction_pointer),
        },
        InterruptMode::Fail => WorkflowInterruptOutcome::Fail {
            message: format!(
                "interrupt handler for '{}' selected fail",
                frame.source.as_str()
            ),
        },
    };
    continuation.status = WorkflowContinuationStatus::Succeeded;
    WorkflowVmStep::InterruptResolved {
        handler: continuation,
        interrupted_continuation_id: frame.interrupted_continuation_id,
        outcome,
    }
}

/// The interrupt frame a handler continuation carries, if this is one.
fn interrupt_frame(continuation: &WorkflowContinuation) -> Option<&WorkflowInterruptFrame> {
    continuation
        .frames
        .iter()
        .rev()
        .find_map(|frame| match frame {
            WorkflowFrame::Interrupt(frame) => Some(frame),
            _ => None,
        })
}

/// Route a failure through the nearest structured try frame, then through the durable
/// compensation stack. This keeps the decision entirely inside the continuation; a host never
/// needs to rediscover graph ancestry from node-run history.
fn handle_failure(
    module: &WorkflowModule,
    continuation: WorkflowContinuation,
    message: String,
) -> WorkflowVmStep {
    handle_classified_failure(module, continuation, WorkflowFailure::failed(message))
}

/// The `on_failure` / `on_timeout` / `on_reject` edges of one graph node are compiled into a try
/// frame whose targets differ by classification, so the routing decision needs the kind and not
/// only the message.
fn handle_classified_failure(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    failure: WorkflowFailure,
) -> WorkflowVmStep {
    let message = failure.message;
    if let Some(position) = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Try(_)))
    {
        let frame = match continuation.frames.remove(position) {
            WorkflowFrame::Try(frame) => frame,
            _ => unreachable!("try frame position was checked"),
        };
        if frame.phase == WorkflowTryPhase::Body {
            let classified = match failure.kind {
                WorkflowFailureKind::TimedOut => frame.on_timeout,
                WorkflowFailureKind::Rejected => frame.on_reject,
                WorkflowFailureKind::Failed | WorkflowFailureKind::Canceled => None,
            };
            if let Some(catch) = classified.or(frame.catch) {
                continuation
                    .frames
                    .push(WorkflowFrame::Try(WorkflowTryFrame {
                        phase: WorkflowTryPhase::Catch,
                        ..frame
                    }));
                continuation.instruction_pointer = catch;
                return step(module, continuation);
            }
        }
        if let Some(finally) = frame.finally {
            continuation
                .frames
                .push(WorkflowFrame::Try(WorkflowTryFrame {
                    phase: WorkflowTryPhase::Finally,
                    pending_failure: Some(message),
                    ..frame
                }));
            continuation.instruction_pointer = finally;
            return step(module, continuation);
        }
    }

    if let Some(position) = continuation
        .frames
        .iter()
        .position(|frame| matches!(frame, WorkflowFrame::Compensation(_)))
    {
        continuation.locals.insert(
            "__workflow_vm_compensation_failure".into(),
            Value::String(message.clone()),
        );
        let frame = match &mut continuation.frames[position] {
            WorkflowFrame::Compensation(frame) => frame,
            _ => unreachable!("compensation frame position was checked"),
        };
        if let Some(request) = frame.pending.pop() {
            frame.active = Some(request.clone());
            return yield_effect(continuation, request);
        }
    }
    fail(continuation, message)
}

fn stable_id(namespace: uuid::Uuid, name: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&namespace, name.as_bytes())
}

fn local_context(continuation: &WorkflowContinuation) -> Value {
    Value::Object(
        continuation
            .locals
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

/// Evaluate selectors that were deliberately kept explicit in bytecode so their ordering and
/// bucket calculation stay reproducible after authoring definitions have changed.
fn select_target(
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

    fn interrupt_module() -> WorkflowModule {
        // [0] enter    [1] check    [2] effect    [3] return
        // [4] handler: const  [5] resume(mode)
        let mut module = WorkflowModule::new(vec![
            WorkflowInstruction::EnterNode {
                node_id: "call".into(),
            },
            WorkflowInstruction::CheckInterrupt {
                handlers: vec![runinator_models::workflow_vm::WorkflowVmInterruptHandler {
                    source: InterruptSource::External,
                    target: 4,
                }],
            },
            WorkflowInstruction::Effect {
                request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
            },
            WorkflowInstruction::Return,
            WorkflowInstruction::Const {
                value: Value::from("handled"),
            },
            WorkflowInstruction::ResumeInterrupt {
                mode: InterruptMode::Resume,
            },
        ]);
        module.source_map = vec![runinator_models::workflow_vm::WorkflowSourceMapEntry {
            version: runinator_models::workflow_vm::WORKFLOW_SOURCE_MAP_VERSION,
            instruction_start: 0,
            instruction_end: 4,
            node_id: "call".into(),
            edge_label: None,
            interruptible: true,
            exit_instruction_pointer: Some(3),
        }];
        module
    }

    #[test]
    fn a_handler_id_is_stable_per_occurrence_but_distinct_across_them() {
        // the engine's retry path starts one handler per attempt. the id must be stable enough that
        // a redelivered result inserts nothing, and distinct enough that attempt 2 still gets a
        // handler of its own.
        let module = WorkflowModule::new(vec![WorkflowInstruction::Return]);
        let continuation = WorkflowContinuation::start(Uuid::now_v7(), module.version);
        let build = |discriminator: &str| {
            interrupt_handler_continuation(
                &module,
                &continuation,
                InterruptSource::Retry,
                Value::Null,
                0,
                discriminator,
            )
            .id
        };

        assert_eq!(build("attempt:1"), build("attempt:1"));
        assert_ne!(build("attempt:1"), build("attempt:2"));
    }

    #[test]
    fn a_handler_reads_what_raised_it_and_leaves_the_thread_alone() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::Return]);
        let continuation = WorkflowContinuation::start(Uuid::now_v7(), module.version);
        let before = continuation.status;
        let handler = interrupt_handler_continuation(
            &module,
            &continuation,
            InterruptSource::Retry,
            runinator_models::json!({ "next_attempt": 2 }),
            0,
            "attempt:2",
        );

        assert_eq!(handler.parent_id, Some(continuation.id));
        assert_eq!(handler.locals["interrupt"]["source"], Value::from("retry"));
        assert_eq!(handler.locals["interrupt"]["payload"]["next_attempt"], 2);
        // building a handler decides nothing about the interrupted thread; the retry path relies on
        // that, because a suspended thread could never be settled by its retried effect.
        assert_eq!(continuation.status, before);
    }

    #[test]
    fn a_requested_interrupt_freezes_its_thread_and_a_handler_runs_beside_it() {
        let module = interrupt_module();
        let mut continuation = continuation();
        continuation.pending_interrupt =
            Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
                id: uuid::Uuid::now_v7(),
                source: InterruptSource::External,
                payload: Value::from("stop"),
            });

        let WorkflowVmStep::Interrupted {
            suspended, handler, ..
        } = step(&module, continuation)
        else {
            panic!("a requested interrupt must suspend its thread");
        };
        assert_eq!(suspended.status, WorkflowContinuationStatus::Suspended);
        // consumed by the drive that decided about it, so it cannot fire again later.
        assert_eq!(suspended.pending_interrupt, None);
        assert_eq!(handler.instruction_pointer, 4);
        assert_eq!(handler.parent_id, Some(suspended.id));
        assert_eq!(
            handler
                .locals
                .get("interrupt")
                .and_then(|v| v.get("source")),
            Some(&Value::from("external"))
        );

        let WorkflowVmStep::InterruptResolved {
            interrupted_continuation_id,
            outcome,
            handler,
        } = step(&module, handler)
        else {
            panic!("the handler must hand control back");
        };
        assert_eq!(interrupted_continuation_id, suspended.id);
        assert_eq!(handler.status, WorkflowContinuationStatus::Succeeded);
        // `resume` continues after the safe point, never at it — otherwise the same interrupt
        // would be re-examined on the very next drive.
        assert_eq!(
            outcome,
            WorkflowInterruptOutcome::Resume {
                instruction_pointer: 2
            }
        );
    }

    #[test]
    fn a_handler_mode_picks_where_the_frozen_thread_lands() {
        let module = interrupt_module();
        for (mode, expected) in [
            (InterruptMode::Resume, Some(2usize)),
            (InterruptMode::Restart, Some(0)),
            (InterruptMode::Continue, Some(3)),
            (InterruptMode::Fail, None),
        ] {
            let mut continuation = continuation();
            continuation.pending_interrupt =
                Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
                    id: uuid::Uuid::now_v7(),
                    source: InterruptSource::External,
                    payload: Value::Null,
                });
            let WorkflowVmStep::Interrupted { handler, .. } = step(&module, continuation) else {
                panic!("expected a suspension");
            };
            let WorkflowVmStep::InterruptResolved { outcome, .. } =
                resolve_interrupt(&module, handler, mode)
            else {
                panic!("{mode:?} must resolve the interrupt");
            };
            match (expected, outcome) {
                (
                    Some(ip),
                    WorkflowInterruptOutcome::Resume {
                        instruction_pointer,
                    },
                ) => {
                    assert_eq!(instruction_pointer, ip, "{mode:?}")
                }
                // a handler can settle the interrupted node failed; it can never fail the run.
                (None, WorkflowInterruptOutcome::Fail { .. }) => {}
                (expected, outcome) => panic!("{mode:?} gave {outcome:?}, wanted {expected:?}"),
            }
        }
    }

    #[test]
    fn a_source_nobody_declared_is_dropped_rather_than_left_pending() {
        let module = interrupt_module();
        let mut continuation = continuation();
        continuation.pending_interrupt =
            Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
                id: uuid::Uuid::now_v7(),
                source: InterruptSource::Child,
                payload: Value::Null,
            });
        let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation) else {
            panic!("an unhandled source must not stop the drive");
        };
        assert_eq!(continuation.pending_interrupt, None);
    }

    #[test]
    fn a_settled_effect_raises_the_source_it_represents() {
        let mut module = interrupt_module();
        module.instructions[1] = WorkflowInstruction::CheckInterrupt {
            handlers: Vec::new(),
        };
        module.interrupt_handlers =
            vec![runinator_models::workflow_vm::WorkflowVmInterruptHandler {
                source: InterruptSource::Wake,
                target: 4,
            }];
        let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
            panic!("the timer must yield");
        };
        let request = WorkflowEffectRequest::TimerDelay { seconds: 1 };
        let WorkflowVmStep::Interrupted { source, .. } =
            resume(&module, continuation, Some(&request), Ok(Value::Null))
        else {
            panic!("an elapsed timer with a `wake` handler must raise it");
        };
        assert_eq!(source, InterruptSource::Wake);
    }

    #[test]
    fn a_handler_that_breaks_gives_the_frozen_thread_back_instead_of_failing_the_run() {
        let mut module = interrupt_module();
        module.instructions[4] = WorkflowInstruction::Fail {
            message: "handler blew up".into(),
        };
        let mut continuation = continuation();
        continuation.pending_interrupt =
            Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
                id: uuid::Uuid::now_v7(),
                source: InterruptSource::External,
                payload: Value::Null,
            });
        let WorkflowVmStep::Interrupted { handler, .. } = step(&module, continuation) else {
            panic!("expected a suspension");
        };
        let WorkflowVmStep::InterruptResolved { outcome, .. } = step(&module, handler) else {
            panic!("a broken handler must still hand control back, not fail the run");
        };
        assert_eq!(
            outcome,
            WorkflowInterruptOutcome::Resume {
                instruction_pointer: 2
            }
        );
    }

    #[test]
    fn a_timeout_takes_the_on_timeout_edge_and_a_plain_failure_the_catch() {
        // the shape the compiler emits for a node carrying both `on_failure` and `on_timeout`:
        // one guard whose classified target is preferred over `catch`.
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::BeginTry {
                try_key: "call#edge".into(),
                catch: Some(5),
                on_timeout: Some(7),
                on_reject: None,
                finally: None,
            },
            WorkflowInstruction::Effect {
                request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
            },
            WorkflowInstruction::EndTry {
                try_key: "call#edge".into(),
            },
            WorkflowInstruction::Const {
                value: Value::from("ok"),
            },
            WorkflowInstruction::Return,
            WorkflowInstruction::EndTry {
                try_key: "call#edge".into(),
            },
            WorkflowInstruction::Fail {
                message: "recovered".into(),
            },
            WorkflowInstruction::EndTry {
                try_key: "call#edge".into(),
            },
            WorkflowInstruction::Fail {
                message: "slow".into(),
            },
        ]);

        for (kind, expected) in [
            (WorkflowFailureKind::TimedOut, "slow"),
            (WorkflowFailureKind::Failed, "recovered"),
            // no `on_reject` edge was compiled, so a rejection falls back to `catch`.
            (WorkflowFailureKind::Rejected, "recovered"),
        ] {
            let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
                panic!("the guarded effect must yield");
            };
            let WorkflowVmStep::Failed { message, .. } = resume(
                &module,
                continuation,
                None,
                Err(WorkflowFailure::new(kind, "boom")),
            ) else {
                panic!("{kind:?} must reach a terminal");
            };
            assert_eq!(message, expected, "{kind:?} took the wrong edge");
        }
    }

    #[test]
    fn freezes_action_references_before_yielding() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Action {
                provider: "test".into(),
                function: "run".into(),
                input: runinator_models::json!({
                    "customer": { "$ref": { "input": ["customer"] } }
                }),
                timeout_seconds: Some(10),
                retry: Default::default(),
                tags: Vec::new(),
                required_labels: Default::default(),
                idempotency_key: Some(runinator_models::json!({
                    "$ref": { "input": ["request_id"] }
                })),
                function_binding: None,
            },
        }]);
        let mut continuation = continuation();
        continuation.locals.insert(
            "input".into(),
            runinator_models::json!({
                "customer": "acme",
                "request_id": "request-7"
            }),
        );

        let result = step(&module, continuation);
        let WorkflowVmStep::Yield { request, .. } = result else {
            panic!("action must yield, got {result:?}");
        };
        let WorkflowEffectRequest::Action {
            input,
            idempotency_key,
            ..
        } = request
        else {
            panic!("expected action request");
        };
        assert_eq!(input, runinator_models::json!({ "customer": "acme" }));
        assert_eq!(idempotency_key, Some(Value::String("request-7".into())));
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
        let WorkflowVmStep::Complete { value, .. } = resume(
            &module,
            continuation,
            None,
            Ok(Value::String("done".into())),
        ) else {
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
            resume(&module, continuation, None, Ok(Value::Null))
        else {
            panic!("expected completion");
        };
        assert!(matches!(
            resume(&module, continuation, None, Ok(Value::Null)),
            WorkflowVmStep::Failed { .. }
        ));
    }

    #[test]
    fn loop_opcode_rejects_a_non_array_initial_value() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::BeginLoop {
            loop_key: "items".into(),
            body: 0,
            exit: 0,
            max_iterations: None,
        }]);
        let WorkflowVmStep::Failed { message, .. } = step(&module, continuation()) else {
            panic!("expected an unsupported-opcode failure");
        };
        assert!(message.contains("needs an array value"));
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
                workflow_revision: None,
                workflow_revision_digest: None,
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
            } = resume(&module, waiting, None, Ok(value.clone()))
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
    fn race_forks_deterministically_and_keeps_winner_state_only_on_the_parent() {
        let module = WorkflowModule::new(vec![WorkflowInstruction::Race {
            targets: vec![1, 2],
            race_key: "fastest".into(),
            winner: runinator_models::workflow_vm::WorkflowBranchPolicy::FirstSuccess,
        }]);
        let WorkflowVmStep::Fork {
            parent,
            children,
            join_key,
        } = step(&module, continuation())
        else {
            panic!("race should fork contenders");
        };
        assert_eq!(join_key, "fastest");
        assert!(parent.frames.iter().any(|frame| matches!(
            frame,
            WorkflowFrame::Race(frame)
                if frame.expected == 2
                    && frame.winner_policy == runinator_models::workflow_vm::WorkflowBranchPolicy::FirstSuccess
                    && frame.winner.is_none()
        )));
        assert!(children.iter().all(|child| {
            child.fork_key.as_deref() == Some("fastest")
                && child
                    .frames
                    .iter()
                    .any(|frame| matches!(frame, WorkflowFrame::Fork(_)))
                && !child
                    .frames
                    .iter()
                    .any(|frame| matches!(frame, WorkflowFrame::Race(_)))
        }));
    }

    #[test]
    fn map_forks_only_its_concurrency_window_with_stable_item_bindings() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Const {
                value: Value::Array(vec![1.into(), 2.into(), 3.into()]),
            },
            WorkflowInstruction::BeginMap {
                map_key: "fanout".into(),
                body: 3,
                exit: 4,
                concurrency: 2,
            },
        ]);
        let WorkflowVmStep::Fork {
            parent,
            children,
            join_key,
        } = step(&module, continuation())
        else {
            panic!("map should fork its initial window");
        };
        assert_eq!(join_key, "fanout");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].locals.get("fanout.item"), Some(&Value::from(1)));
        assert_eq!(
            children[1].locals.get("fanout.index"),
            Some(&Value::from(1))
        );
        assert!(parent.frames.iter().any(|frame| matches!(
            frame, WorkflowFrame::Map(frame)
                if frame.next_index == 2 && frame.items.len() == 3 && frame.results.is_empty()
        )));
        assert!(
            children
                .iter()
                .all(|child| child.frames.iter().any(|frame| matches!(
                    frame, WorkflowFrame::Map(frame) if frame.item_index.is_some()
                )))
        );
    }

    #[test]
    fn map_child_arrival_reports_its_body_result_not_the_re_evaluated_items() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Const {
                value: Value::Array(vec![1.into()]),
            },
            WorkflowInstruction::BeginMap {
                map_key: "fanout".into(),
                body: 2,
                exit: 5,
                concurrency: 1,
            },
            WorkflowInstruction::Const {
                value: Value::String("body output".into()),
            },
            WorkflowInstruction::Const {
                value: Value::Array(vec![1.into()]),
            },
            WorkflowInstruction::Jump { target: 1 },
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Fork { children, .. } = step(&module, continuation()) else {
            panic!("map should fork its item");
        };
        let WorkflowVmStep::Joined {
            join_key, value, ..
        } = step(&module, children.into_iter().next().unwrap())
        else {
            panic!("map child should arrive at the map coordinator");
        };
        assert_eq!(join_key, "fanout");
        assert_eq!(value, Value::String("body output".into()));
    }

    #[test]
    fn loop_frame_survives_reentry_and_collects_results_in_order() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Const {
                value: Value::Array(vec![Value::from(1), Value::from(2)]),
            },
            WorkflowInstruction::BeginLoop {
                loop_key: "items".into(),
                body: 5,
                exit: 4,
                max_iterations: None,
            },
            WorkflowInstruction::Fail {
                message: "unreachable".into(),
            },
            WorkflowInstruction::Fail {
                message: "unreachable".into(),
            },
            WorkflowInstruction::Return,
            WorkflowInstruction::Const {
                value: Value::String("item".into()),
            },
            // This mirrors the compiler's item expression on a graph re-entry. BeginLoop
            // discards it while retaining the body result below it.
            WorkflowInstruction::Const {
                value: Value::Array(vec![Value::from(1), Value::from(2)]),
            },
            WorkflowInstruction::Jump { target: 1 },
        ]);
        let WorkflowVmStep::Complete {
            value,
            continuation,
        } = step(&module, continuation())
        else {
            panic!("loop should finish");
        };
        assert_eq!(
            value,
            Value::Array(vec![
                Value::String("item".into()),
                Value::String("item".into())
            ])
        );
        assert!(
            continuation
                .frames
                .iter()
                .all(|frame| !matches!(frame, WorkflowFrame::Loop(_)))
        );
    }

    #[test]
    fn failure_unwinds_compensation_before_the_terminal_failure() {
        let action = WorkflowEffectRequest::Timer { due_at: 1 };
        let compensation = WorkflowEffectRequest::Timer { due_at: 2 };
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Effect { request: action },
            WorkflowInstruction::RegisterCompensation {
                compensation_key: "charge".into(),
                request: compensation.clone(),
            },
            WorkflowInstruction::Fail {
                message: "charge failed".into(),
            },
            WorkflowInstruction::BeginCompensation { resume: None },
        ]);
        let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
            panic!("main action should yield")
        };
        let WorkflowVmStep::Yield {
            continuation,
            request,
            ..
        } = resume(&module, continuation, None, Ok(Value::Null))
        else {
            panic!("compensation should yield")
        };
        assert_eq!(request, compensation);
        let WorkflowVmStep::Failed {
            message,
            continuation,
        } = resume(&module, continuation, None, Ok(Value::Null))
        else {
            panic!("failure should survive compensation")
        };
        assert_eq!(message, "charge failed");
        assert_eq!(continuation.status, WorkflowContinuationStatus::Failed);
    }

    #[test]
    fn try_catch_finally_unwinds_through_the_same_continuation() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::BeginTry {
                try_key: "guard".into(),
                catch: Some(4),
                on_timeout: None,
                on_reject: None,
                finally: Some(6),
            },
            WorkflowInstruction::Jump { target: 3 },
            WorkflowInstruction::Return,
            WorkflowInstruction::Fail {
                message: "body failed".into(),
            },
            WorkflowInstruction::Const {
                value: Value::String("caught".into()),
            },
            WorkflowInstruction::Jump { target: 0 },
            WorkflowInstruction::Const {
                value: Value::String("finally".into()),
            },
            WorkflowInstruction::Jump { target: 0 },
        ]);
        let WorkflowVmStep::Complete {
            value,
            continuation,
        } = step(&module, continuation())
        else {
            panic!("catch/finally should complete")
        };
        assert_eq!(value, Value::String("finally".into()));
        assert!(
            continuation
                .frames
                .iter()
                .all(|frame| !matches!(frame, WorkflowFrame::Try(_)))
        );
    }

    #[test]
    fn compiled_linear_graph_reaches_the_same_terminal_result() {
        let module = compile_workflow_module(&WorkflowDefinition {
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

    #[test]
    fn toggle_selector_selects_a_boolean_target() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Select {
                kind: WorkflowNodeKind::Toggle,
                configuration: serde_json::json!({
                    "parameters": { "value": true }
                })
                .into(),
                targets: vec![1, 3],
                default: None,
            },
            WorkflowInstruction::Const {
                value: Value::String("on".into()),
            },
            WorkflowInstruction::Jump { target: 4 },
            WorkflowInstruction::Const {
                value: Value::String("off".into()),
            },
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Complete { value, .. } = step(&module, continuation()) else {
            panic!("toggle should select a branch");
        };
        assert_eq!(value, Value::String("on".into()));
    }

    #[test]
    fn output_and_debug_opcodes_are_executable() {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Const {
                value: Value::String("result".into()),
            },
            WorkflowInstruction::SetOutput {
                event_type: Some("finished".into()),
                artifacts: vec![],
            },
            WorkflowInstruction::DebugBoundary {
                label: Some("after-output".into()),
            },
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Complete {
            continuation,
            value,
        } = step(&module, continuation())
        else {
            panic!("output and debug instructions should not be unsupported");
        };
        assert_eq!(value, Value::String("result".into()));
        assert_eq!(
            continuation
                .locals
                .get("__workflow_vm_output")
                .and_then(|value| value.get("event_type")),
            Some(&Value::String("finished".into()))
        );
        assert!(continuation.frames.iter().any(|frame| matches!(frame, WorkflowFrame::Debug(frame) if frame.breakpoint.as_deref() == Some("after-output"))));
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
