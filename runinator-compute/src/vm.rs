//! the resumable invocation vm.
//!
//! it runs an [`InvocationProgram`] over an operand stack until one of four things happens: the
//! program finishes, it fails, it jumps the cursor, or it reaches a call it cannot make here. the
//! last case is the whole point — the vm hands back the call plus a serializable
//! [`InvocationContinuation`], and a later `resume` picks up at exactly that instruction with the
//! call's result on the stack.
//!
//! what makes that resumable is that *all* execution state lives in the continuation: the frame
//! stack, each frame's instruction pointer, operand stack and locals. nothing is held in rust
//! call frames across a yield, which is why the vm is a loop over an explicit stack rather than a
//! recursive evaluator like the one it replaces.

use runinator_models::invocation::{
    CallableTarget, ClosureCell, EffectClass, InvocationContinuation, InvocationEffect,
    InvocationEffectResult, InvocationFrame, InvocationInstruction, InvocationModule,
    InvocationProgram, InvocationStep, RecordedLocal, closure_handle, closure_handle_index,
};
use runinator_models::value::Value;
use runinator_models::workflow_ast::WorkflowValueRef;

use crate::catalog::CallableCatalog;
use crate::compute::{IntrinsicLibrary, call_pure};
use crate::errors::WorkflowValidationError;
use crate::expressions::resolve_value_ref;

/// the ceiling on nested frames, mirroring the evaluator's old `MAX_CALL_DEPTH`.
///
/// unlike the old one this counts *every* frame, not only user-function calls: a recursive closure
/// used to be bounded by nothing but the rust stack, because lambda application never bumped the
/// depth. an explicit frame stack makes that a real limit rather than a segfault.
pub const MAX_FRAME_DEPTH: usize = 1024;

/// how many instructions one `step`/`resume` will run before giving up.
///
/// a malformed jump can loop forever without ever calling anything, and a wedged reducer is much
/// worse than a failed invocation.
pub const MAX_INSTRUCTIONS_PER_STEP: usize = 1_000_000;

/// everything the vm needs that is not the program itself.
pub struct VmEnv<'a> {
    /// the run context `$ref` instructions resolve against.
    pub context: &'a Value,
    /// what is callable, and how far each call's result can travel.
    pub catalog: &'a CallableCatalog,
    /// the library backing `Local` intrinsics (`now`/`uuid`/`env`).
    ///
    /// `None` in a pure-only setting, which makes a local call an error rather than a silent
    /// observation of the host.
    pub locals: Option<&'a dyn IntrinsicLibrary>,
}

impl<'a> VmEnv<'a> {
    /// an environment that can fold pure calls and nothing else.
    pub fn pure(context: &'a Value, catalog: &'a CallableCatalog) -> Self {
        Self {
            context,
            catalog,
            locals: None,
        }
    }

    /// an environment that can also observe the host for `Local` intrinsics.
    pub fn with_locals(
        context: &'a Value,
        catalog: &'a CallableCatalog,
        locals: &'a dyn IntrinsicLibrary,
    ) -> Self {
        Self {
            context,
            catalog,
            locals: Some(locals),
        }
    }
}

/// start a module's entry program.
pub fn start(module: &InvocationModule, env: &VmEnv<'_>) -> InvocationStep {
    if !module.is_supported() {
        return InvocationStep::Failed {
            message: format!(
                "invocation module version {} is not supported by this runtime",
                module.version
            ),
        };
    }
    let continuation = InvocationContinuation::start();
    run(module, continuation, env)
}

/// continue a suspended program from its continuation, without supplying a call result.
///
/// this is what a fresh process does after loading a stored continuation whose call is still in
/// flight; it is a no-op step that re-validates the module version.
pub fn step(
    module: &InvocationModule,
    continuation: InvocationContinuation,
    env: &VmEnv<'_>,
) -> InvocationStep {
    if let Some(failure) = version_mismatch(module, &continuation) {
        return failure;
    }
    run(module, continuation, env)
}

/// resume a suspended program with the outcome of the call it yielded on.
pub fn resume(
    module: &InvocationModule,
    mut continuation: InvocationContinuation,
    result: InvocationEffectResult,
    env: &VmEnv<'_>,
) -> InvocationStep {
    if let Some(failure) = version_mismatch(module, &continuation) {
        return failure;
    }
    let value = match result {
        InvocationEffectResult::Ok { value } => value,
        // the call already exhausted its own retry policy, so this fails the invocation and the
        // node's normal failure transition takes over from there.
        InvocationEffectResult::Failed { message } => return InvocationStep::Failed { message },
    };
    let Some(frame) = continuation.frames.last_mut() else {
        return InvocationStep::Failed {
            message: "resumed a continuation with no frames".to_string(),
        };
    };
    if !frame.awaiting {
        return InvocationStep::Failed {
            message: "resumed a continuation that was not awaiting a call".to_string(),
        };
    }
    frame.awaiting = false;
    frame.stack.push(value);
    run(module, continuation, env)
}

/// run a program to completion, refusing to yield.
///
/// this is the entry point for every position that must answer now — conditions, defaults,
/// parameter resolution, an editor preview. a durable call in one of those positions is a compile
/// error by the time it gets here, so reaching one is a bug worth reporting loudly rather than a
/// case to handle.
pub fn evaluate_pure(
    program: &InvocationProgram,
    context: &Value,
    catalog: &CallableCatalog,
) -> Result<Value, WorkflowValidationError> {
    let module = InvocationModule::new(program.clone());
    let env = VmEnv::pure(context, catalog);
    match start(&module, &env) {
        InvocationStep::Complete { value } => Ok(value),
        InvocationStep::Failed { message } => {
            Err(WorkflowValidationError::InvalidComputeProgram(message))
        }
        InvocationStep::Yield { effect, .. } => {
            Err(WorkflowValidationError::InvalidComputeProgram(format!(
                "'{}' cannot be called here: this position must be evaluated without dispatching",
                effect.target.display_name()
            )))
        }
        InvocationStep::Goto { .. } => Err(WorkflowValidationError::InvalidComputeProgram(
            "goto is not allowed in an expression position".to_string(),
        )),
    }
}

// a stored continuation is only meaningful to the vm version that produced it.
fn version_mismatch(
    module: &InvocationModule,
    continuation: &InvocationContinuation,
) -> Option<InvocationStep> {
    if !module.is_supported() || continuation.version != module.version {
        return Some(InvocationStep::Failed {
            message: format!(
                "continuation version {} does not match module version {}",
                continuation.version, module.version
            ),
        });
    }
    None
}

// the interpreter loop. every early return is one of the four step outcomes.
fn run(
    module: &InvocationModule,
    mut continuation: InvocationContinuation,
    env: &VmEnv<'_>,
) -> InvocationStep {
    let mut budget = MAX_INSTRUCTIONS_PER_STEP;
    loop {
        if budget == 0 {
            return failed("invocation exceeded its instruction budget");
        }
        budget -= 1;

        let Some(frame) = continuation.frames.last() else {
            // every frame returned: the entry program's value is the invocation's value. `run`
            // pops the last frame only through `Return`, which stores the value first.
            return failed("invocation ran out of frames without returning");
        };
        if frame.awaiting {
            return failed("stepped a continuation that is awaiting a call");
        }

        let program = match current_program(module, frame) {
            Ok(program) => program,
            Err(step) => return step,
        };

        let Some(instruction) = program.get(frame.ip).cloned() else {
            // falling off the end of a program returns null, which is what an author writing a
            // block with no `return` means.
            match pop_frame(&mut continuation, Value::Null) {
                FrameExit::Completed(value) => return InvocationStep::Complete { value },
                FrameExit::Returned => continue,
            }
        };

        match execute(&instruction, module, &mut continuation, env) {
            Ok(Flow::Next) => {
                if let Some(frame) = continuation.frames.last_mut() {
                    frame.ip += 1;
                }
            }
            Ok(Flow::Jumped) => {}
            Ok(Flow::Step(step)) => return step,
            Err(message) => return InvocationStep::Failed { message },
        }
    }
}

// what executing one instruction did to control flow.
enum Flow {
    /// advance to the next instruction.
    Next,
    /// the instruction already set the instruction pointer.
    Jumped,
    /// the invocation is done with this step.
    Step(InvocationStep),
}

// popping a frame either returns into a caller or finishes the invocation.
enum FrameExit {
    Completed(Value),
    Returned,
}

fn pop_frame(continuation: &mut InvocationContinuation, value: Value) -> FrameExit {
    continuation.frames.pop();
    match continuation.frames.last_mut() {
        Some(caller) => {
            caller.stack.push(value);
            // the caller's `Call`/`Apply` instruction is finished, so control resumes after it.
            caller.ip += 1;
            FrameExit::Returned
        }
        None => FrameExit::Completed(value),
    }
}

fn current_program<'m, 'f: 'm>(
    module: &'m InvocationModule,
    frame: &'f InvocationFrame,
) -> Result<&'m InvocationProgram, InvocationStep> {
    // an inline body wins: a closure frame carries the only copy of what it runs.
    if let Some(body) = &frame.body {
        return Ok(body);
    }
    match &frame.function {
        None => Ok(&module.entry),
        Some(name) => match module.function(name) {
            Some(function) => Ok(&function.body),
            None => Err(failed(format!("unknown function '{name}' in continuation"))),
        },
    }
}

fn execute(
    instruction: &InvocationInstruction,
    module: &InvocationModule,
    continuation: &mut InvocationContinuation,
    env: &VmEnv<'_>,
) -> Result<Flow, String> {
    match instruction {
        InvocationInstruction::Const { value } => {
            push(continuation, value.clone())?;
            Ok(Flow::Next)
        }
        InvocationInstruction::LoadRef { reference } => {
            let parsed = WorkflowValueRef::try_from(reference)
                .map_err(|err| format!("invalid reference: {err}"))?;
            let value = resolve_value_ref(&parsed, env.context).map_err(|err| err.to_string())?;
            push(continuation, value)?;
            Ok(Flow::Next)
        }
        InvocationInstruction::LoadLocal { name } => {
            let value = frame(continuation)?
                .locals
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .ok_or_else(|| format!("unknown local '{name}'"))?;
            push(continuation, value)?;
            Ok(Flow::Next)
        }
        InvocationInstruction::StoreLocal { name } => {
            let value = pop(continuation)?;
            frame_mut(continuation)?.locals.push((name.clone(), value));
            Ok(Flow::Next)
        }
        InvocationInstruction::Pop => {
            pop(continuation)?;
            Ok(Flow::Next)
        }
        InvocationInstruction::Jump { target } => {
            frame_mut(continuation)?.ip = *target;
            Ok(Flow::Jumped)
        }
        InvocationInstruction::JumpIfFalse { target } => {
            let value = pop(continuation)?;
            if truthy(&value) {
                Ok(Flow::Next)
            } else {
                frame_mut(continuation)?.ip = *target;
                Ok(Flow::Jumped)
            }
        }
        InvocationInstruction::Return => {
            let value = pop(continuation)?;
            match pop_frame(continuation, value) {
                FrameExit::Completed(value) => Ok(Flow::Step(InvocationStep::Complete { value })),
                FrameExit::Returned => Ok(Flow::Jumped),
            }
        }
        InvocationInstruction::Goto { target } => Ok(Flow::Step(InvocationStep::Goto {
            target: target.clone(),
        })),
        InvocationInstruction::Closure { params, body } => {
            let captured = frame(continuation)?.locals.clone();
            let index = continuation.closures.len();
            continuation.closures.push(ClosureCell {
                params: params.clone(),
                body: body.clone(),
                captured,
            });
            push(continuation, closure_handle(index))?;
            Ok(Flow::Next)
        }
        InvocationInstruction::Apply { argc } => apply(continuation, *argc),
        InvocationInstruction::Call {
            target,
            argc,
            names,
            policy,
        } => call(continuation, module, env, target, *argc, names, policy),
    }
}

#[allow(clippy::too_many_arguments)]
fn call(
    continuation: &mut InvocationContinuation,
    module: &InvocationModule,
    env: &VmEnv<'_>,
    target: &CallableTarget,
    argc: usize,
    names: &[String],
    policy: &Option<runinator_models::invocation::CallPolicy>,
) -> Result<Flow, String> {
    let args = pop_n(continuation, argc)?;

    // a call over a secret placeholder must not be folded here: the value is the literal
    // `secret://…` text until a worker substitutes it, so an in-process answer would be wrong
    // rather than merely late.
    let over_secret = args.iter().any(crate::catalog::contains_secret_reference);

    let effect = match target {
        CallableTarget::Intrinsic { name } => env.catalog.effect_of(name),
        CallableTarget::Local { .. } => EffectClass::Pure,
        CallableTarget::Provider { .. } | CallableTarget::Packaged { .. } => EffectClass::Durable,
    };

    if over_secret || effect == EffectClass::Durable || effect == EffectClass::Unknown {
        return Ok(Flow::Step(yield_call(
            continuation,
            target.clone(),
            args,
            names.to_vec(),
            policy.clone().unwrap_or_default(),
        )));
    }

    match target {
        CallableTarget::Local { name } => enter_function(continuation, module, name, args),
        CallableTarget::Intrinsic { name } => {
            let value = match effect {
                EffectClass::Local => observe_local(continuation, env, name, &args)?,
                _ => intrinsic(env, name, &args)?,
            };
            push(continuation, value)?;
            Ok(Flow::Next)
        }
        // already handled above.
        CallableTarget::Provider { .. } | CallableTarget::Packaged { .. } => {
            Err("durable target reached the in-process path".to_string())
        }
    }
}

// suspend the current frame on a call, recording where to put the result.
fn yield_call(
    continuation: &mut InvocationContinuation,
    target: CallableTarget,
    args: Vec<Value>,
    names: Vec<String>,
    policy: runinator_models::invocation::CallPolicy,
) -> InvocationStep {
    let sequence = continuation.call_sequence;
    continuation.call_sequence += 1;
    if let Some(frame) = continuation.frames.last_mut() {
        frame.awaiting = true;
        // the continuation is saved *after* the call instruction: the call is what suspended us, so
        // resuming must land on what follows it. leaving `ip` on the call would re-dispatch it and
        // the invocation would never make progress.
        frame.ip += 1;
    }
    InvocationStep::Yield {
        effect: Box::new(InvocationEffect {
            sequence,
            target,
            args,
            names,
            policy,
        }),
        continuation: Box::new(continuation.clone()),
    }
}

// a `Local` intrinsic: replay a recorded value when there is one, otherwise observe and record.
//
// recording is what keeps a resumed run, a debugger replay and a shadow cursor agreeing about what
// time it was — without it, every resume would re-read the clock and the run would tell a different
// story each time it was inspected.
fn observe_local(
    continuation: &mut InvocationContinuation,
    env: &VmEnv<'_>,
    name: &str,
    args: &[Value],
) -> Result<Value, String> {
    let sequence = continuation.call_sequence;
    continuation.call_sequence += 1;
    if let Some(recorded) = continuation
        .recorded
        .iter()
        .find(|entry| entry.sequence == sequence)
    {
        return Ok(recorded.value.clone());
    }
    let library = env
        .locals
        .ok_or_else(|| format!("'{name}' cannot be evaluated in a pure-only position"))?;
    let value = library.call(name, args).map_err(|err| err.to_string())?;
    continuation.recorded.push(RecordedLocal {
        sequence,
        name: name.to_string(),
        value: value.clone(),
    });
    Ok(value)
}

fn intrinsic(env: &VmEnv<'_>, name: &str, args: &[Value]) -> Result<Value, String> {
    if let Some(library) = env.locals
        && library.knows(name)
    {
        return library.call(name, args).map_err(|err| err.to_string());
    }
    call_pure(name, args).map_err(|err| err.to_string())
}

fn enter_function(
    continuation: &mut InvocationContinuation,
    module: &InvocationModule,
    name: &str,
    args: Vec<Value>,
) -> Result<Flow, String> {
    let function = module
        .function(name)
        .ok_or_else(|| format!("unknown function '{name}'"))?;
    if args.len() != function.params.len() {
        return Err(format!(
            "'{name}' takes {} argument(s) but got {}",
            function.params.len(),
            args.len()
        ));
    }
    let depth_cap = function
        .max_depth
        .map(|max| max as usize)
        .unwrap_or(MAX_FRAME_DEPTH)
        .min(MAX_FRAME_DEPTH);
    let same_function = continuation
        .frames
        .iter()
        .filter(|frame| frame.function.as_deref() == Some(name))
        .count();
    if same_function >= depth_cap || continuation.frames.len() >= MAX_FRAME_DEPTH {
        return Err(format!("'{name}' exceeded its recursion limit"));
    }
    let locals = function
        .params
        .iter()
        .cloned()
        .zip(args)
        .collect::<Vec<_>>();
    continuation
        .frames
        .push(InvocationFrame::for_function(name, locals));
    Ok(Flow::Jumped)
}

// apply a closure value: its captured environment plus its parameters become the new frame.
fn apply(continuation: &mut InvocationContinuation, argc: usize) -> Result<Flow, String> {
    let args = pop_n(continuation, argc)?;
    let callee = pop(continuation)?;
    let index = closure_handle_index(&callee).ok_or_else(|| "value is not callable".to_string())?;
    let cell = continuation
        .closures
        .get(index)
        .cloned()
        .ok_or_else(|| "closure handle does not resolve".to_string())?;
    if args.len() != cell.params.len() {
        return Err(format!(
            "closure takes {} argument(s) but got {}",
            cell.params.len(),
            args.len()
        ));
    }
    // unlike the evaluator this replaces, applying a closure costs a real frame, so a recursive
    // closure hits the depth limit instead of the rust stack.
    if continuation.frames.len() >= MAX_FRAME_DEPTH {
        return Err("closure application exceeded the frame depth limit".to_string());
    }
    let mut locals = cell.captured;
    locals.extend(cell.params.into_iter().zip(args));
    continuation
        .frames
        .push(InvocationFrame::for_closure(cell.body, locals));
    Ok(Flow::Jumped)
}

// --- small stack helpers -------------------------------------------------------------------

fn frame<'c>(continuation: &'c InvocationContinuation) -> Result<&'c InvocationFrame, String> {
    continuation
        .frames
        .last()
        .ok_or_else(|| "no active frame".to_string())
}

fn frame_mut<'c>(
    continuation: &'c mut InvocationContinuation,
) -> Result<&'c mut InvocationFrame, String> {
    continuation
        .frames
        .last_mut()
        .ok_or_else(|| "no active frame".to_string())
}

fn push(continuation: &mut InvocationContinuation, value: Value) -> Result<(), String> {
    frame_mut(continuation)?.stack.push(value);
    Ok(())
}

fn pop(continuation: &mut InvocationContinuation) -> Result<Value, String> {
    frame_mut(continuation)?
        .stack
        .pop()
        .ok_or_else(|| "operand stack underflow".to_string())
}

fn pop_n(continuation: &mut InvocationContinuation, count: usize) -> Result<Vec<Value>, String> {
    let frame = frame_mut(continuation)?;
    if frame.stack.len() < count {
        return Err("operand stack underflow".to_string());
    }
    let at = frame.stack.len() - count;
    Ok(frame.stack.split_off(at))
}

fn failed(message: impl Into<String>) -> InvocationStep {
    InvocationStep::Failed {
        message: message.into(),
    }
}

// the one truthiness rule. the evaluator this replaces had three subtly different ones, in
// `conditions.rs`, `expressions.rs` and `compute.rs`; only null and false are falsy.
fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
}

#[cfg(test)]
#[path = "vm_tests.rs"]
mod tests;
