//! Host-free interpreter for [`runinator_models::workflow_vm::WorkflowModule`].
//!
//! The machine stops at durable boundaries. Its caller is responsible for assigning effect ids and
//! atomically persisting the returned continuation and effect receipt.

use runinator_models::{
    value::Value,
    workflow_vm::{
        WorkflowCompensationFrame, WorkflowContinuation, WorkflowContinuationStatus,
        WorkflowEffectRequest, WorkflowForkFrame, WorkflowFrame, WorkflowInstruction,
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
}

/// Resume a continuation after the host durably settled its sole outstanding effect.
pub fn resume(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    result: Result<Value, String>,
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
    match result {
        Ok(value) => continuation.stack.push(value),
        Err(message) => return handle_failure(module, continuation, message),
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
            // Interrupt delivery is a host operation: this safe point deliberately has no
            // side effect until a host creates a handler continuation.  It is nevertheless an
            // executable opcode, not an unsupported persisted state.
            WorkflowInstruction::CheckInterrupt { .. } => continuation.instruction_pointer += 1,
            WorkflowInstruction::ResumeInterrupt { mode } => {
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
                    _ => unreachable!(),
                };
                match mode {
                    runinator_models::interrupt::InterruptMode::Fail => {
                        return handle_failure(
                            module,
                            continuation,
                            "interrupt handler selected fail".into(),
                        );
                    }
                    runinator_models::interrupt::InterruptMode::Resume
                    | runinator_models::interrupt::InterruptMode::Restart => {
                        continuation.instruction_pointer = frame.resume_instruction_pointer;
                    }
                    runinator_models::interrupt::InterruptMode::Continue => {
                        continuation.instruction_pointer += 1
                    }
                }
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
            input,
            wait,
            reuse_open_run,
            run_name,
        } => WorkflowEffectRequest::ChildRun {
            workflow_id,
            workflow_name,
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

/// Route a failure through the nearest structured try frame, then through the durable
/// compensation stack. This keeps the decision entirely inside the continuation; a host never
/// needs to rediscover graph ancestry from node-run history.
fn handle_failure(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    message: String,
) -> WorkflowVmStep {
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
            if let Some(catch) = frame.catch {
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
        } = resume(&module, continuation, Ok(Value::Null))
        else {
            panic!("compensation should yield")
        };
        assert_eq!(request, compensation);
        let WorkflowVmStep::Failed {
            message,
            continuation,
        } = resume(&module, continuation, Ok(Value::Null))
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
