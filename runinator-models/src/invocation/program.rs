//! the stack-oriented program form and the continuation that freezes a run of it.

use super::*;

/// a program is a flat instruction sequence over an operand stack.
///
/// flat rather than a tree because a continuation has to name a *resume point*, and an index into a
/// vector is a stable name that survives serialization. a tree would need a path, and every edit to
/// the shape of the tree would change what that path meant.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InvocationProgram {
    pub instructions: Vec<InvocationInstruction>,
}

impl InvocationProgram {
    pub fn new(instructions: Vec<InvocationInstruction>) -> Self {
        Self { instructions }
    }

    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    pub fn get(&self, ip: usize) -> Option<&InvocationInstruction> {
        self.instructions.get(ip)
    }
}

/// one instruction.
///
/// the set is deliberately small: everything the surface language offers is either a value, a name,
/// a call, or a jump. keeping it small is what makes the vm auditable and the continuation cheap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum InvocationInstruction {
    /// push a constant.
    Const { value: Value },
    /// Pop `len` values and build an array in source order.
    Array { len: usize },
    /// Pop one value for each key and build an object in source order.
    Object { keys: Vec<String> },
    /// resolve a reference against the context and push it.
    LoadRef { reference: Value },
    /// push a local binding's current value.
    LoadLocal { name: String },
    /// pop a value and bind it to a local.
    StoreLocal { name: String },
    /// pop `argc` values (left to right) and call `target`, pushing the result.
    ///
    /// a pure or local call resolves in place; a durable one is where the vm yields.
    Call {
        target: CallableTarget,
        argc: usize,
        /// named-argument labels, positionally aligned with the trailing arguments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        names: Vec<String>,
        /// the call-site policy from a `with { … }` postfix, when the author wrote one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        policy: Option<Box<CallPolicy>>,
    },
    /// Apply a closure across a collection. The VM keeps the iteration state in the continuation so
    /// a lambda may itself suspend on a durable call.
    HigherOrder { name: String, argc: usize },
    /// pop a callee value (a closure) and `argc` arguments, apply, push the result.
    Apply { argc: usize },
    /// push a closure capturing the current locals.
    Closure {
        params: Vec<String>,
        body: InvocationProgram,
    },
    /// pop a value; jump to `target` when it is falsy.
    JumpIfFalse { target: usize },
    /// jump unconditionally.
    Jump { target: usize },
    /// pop a value and return it from the program.
    Return,
    /// move this thread of control to a graph node.
    Goto { target: String },
    /// discard the top of the stack.
    Pop,
}

/// what a call names.
///
/// the four cases are not interchangeable at runtime — an intrinsic folds in place, a local is a
/// module function, a provider is a broker dispatch, and a packaged function additionally carries
/// the binding that pins it to exact published bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallableTarget {
    /// a `std` library function.
    Intrinsic { name: String },
    /// a function defined in this module.
    Local { name: String },
    /// a provider action, dispatched through the broker.
    Provider { provider: String, function: String },
    /// a published packaged function.
    ///
    /// the binding is carried whole rather than copied field-by-field: it is already the single
    /// source for the pinned version and digest, and decompile renders the call back from it
    /// without consulting a catalog, so a definition reads the same after its package is deleted.
    Packaged { binding: FunctionBinding },
}

impl CallableTarget {
    /// the name an author would recognize, for diagnostics.
    pub fn display_name(&self) -> String {
        match self {
            Self::Intrinsic { name } | Self::Local { name } => name.clone(),
            Self::Provider { provider, function } => format!("{provider}.{function}"),
            Self::Packaged { binding } => {
                format!("{}.{}", binding.provider_name(), binding.export_name)
            }
        }
    }

    /// the packaged binding this target pins, when it is a packaged call.
    pub fn binding(&self) -> Option<&FunctionBinding> {
        match self {
            Self::Packaged { binding } => Some(binding),
            _ => None,
        }
    }
}

/// how far a value can travel from where it is computed.
///
/// the ordering matters: a program's class is the strongest class of anything in it, so this is a
/// lattice with `Pure` at the bottom. `Unknown` is *not* the top — it is "cannot be decided
/// statically", which is why it is rejected in pure-only positions rather than being treated as
/// durable and dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    /// no effect and no observation: same inputs, same answer, anywhere.
    Pure,
    /// observes the host but reaches nothing external — `now`, `UUID`, `env`.
    ///
    /// evaluated in the reducer and *recorded* in the continuation, so a replay, a debugger step,
    /// or a shadow cursor sees the value the real run saw rather than a fresh one.
    Local,
    /// reaches something outside the process, so it must become a durable, retryable call.
    Durable,
    /// a first-class function parameter whose effect depends on what gets passed. legal in a
    /// durable program, rejected where only pure code is allowed.
    Unknown,
}

impl EffectClass {
    /// the class of a program containing both.
    pub fn join(self, other: Self) -> Self {
        self.max(other)
    }

    /// whether this can be evaluated to completion inside the reducer.
    pub fn is_in_process(self) -> bool {
        matches!(self, Self::Pure | Self::Local)
    }
}

/// per-call overrides for the enclosing node's policy.
///
/// every field is optional because the node supplies the defaults; a `with { … }` postfix only says
/// what differs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CallPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<CallRetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// an expression, resolved against the run context at dispatch — not a literal, because the key
    /// usually names something about the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Value>,
}

impl CallPolicy {
    /// whether this policy says nothing at all.
    pub fn is_empty(&self) -> bool {
        self.timeout_seconds.is_none()
            && self.retry.is_none()
            && self.runner.is_none()
            && self.tags.is_empty()
            && self.idempotency_key.is_none()
    }

    /// overlay `self` onto `base`, field by field: a call-site value wins, an absent one inherits.
    pub fn overlay(&self, base: &CallPolicy) -> CallPolicy {
        CallPolicy {
            timeout_seconds: self.timeout_seconds.or(base.timeout_seconds),
            retry: self.retry.clone().or_else(|| base.retry.clone()),
            runner: self.runner.clone().or_else(|| base.runner.clone()),
            tags: if self.tags.is_empty() {
                base.tags.clone()
            } else {
                self.tags.clone()
            },
            idempotency_key: self
                .idempotency_key
                .clone()
                .or_else(|| base.idempotency_key.clone()),
        }
    }
}

/// the retry shape a call carries, mirroring the node-level `WorkflowRetry` fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRetry {
    pub max_attempts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_base_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_max_seconds: Option<i64>,
    #[serde(default)]
    pub jitter: bool,
    /// which terminal statuses are retryable, by the same names the node-level policy uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_on: Option<String>,
}

/// a frozen program run: enough to resume it exactly where it stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationContinuation {
    /// the ir version of the module this was frozen against.
    pub version: u32,
    /// the call stack, outermost first. the last frame is where execution resumes.
    pub frames: Vec<InvocationFrame>,
    /// values recorded for `Local` calls already made, keyed by their call sequence, so a resume
    /// replays them instead of observing the host a second time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recorded: Vec<RecordedLocal>,
    /// closures built during this run.
    ///
    /// a closure has to be a *value* — it sits on the operand stack and is passed to `map` — but a
    /// program is not a `Value`. so the closure lives here, typed, and the stack carries a handle
    /// (`{"$closure": <index>}`) into this table. that keeps the body a real `InvocationProgram`
    /// instead of json that would have to be re-parsed on every application, which is what the
    /// evaluator this replaces did.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closures: Vec<ClosureCell>,
    /// how many calls this invocation has made, which is what names the next one.
    #[serde(default)]
    pub call_sequence: i64,
}

impl InvocationContinuation {
    /// a continuation positioned at the start of a module's entry program.
    pub fn start() -> Self {
        Self {
            version: INVOCATION_IR_VERSION,
            frames: vec![InvocationFrame::entry()],
            recorded: Vec::new(),
            closures: Vec::new(),
            call_sequence: 0,
        }
    }

    /// the frame execution resumes in.
    pub fn current(&self) -> Option<&InvocationFrame> {
        self.frames.last()
    }
}

/// one call frame: where it is, what it has computed, and what it is waiting for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationFrame {
    /// which program this frame runs: the module entry, or a named function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// an inline program this frame runs instead of a module function — a closure body.
    ///
    /// a closure is not addressable by name, so a frame running one has to carry it. it is stored
    /// rather than re-derived because the continuation must be resumable in a process that never
    /// saw the closure being built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<InvocationProgram>,
    /// the next instruction to execute.
    pub ip: usize,
    /// the operand stack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<Value>,
    /// lexical locals visible here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locals: Vec<(String, Value)>,
    /// set when this frame is parked on a call; the result is pushed here on resume.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub awaiting: bool,
    /// The higher-order operation this closure frame returns into, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub higher_order: Option<HigherOrderState>,
    /// Named functions are hermetic: references outside their frame locals do not see the caller's
    /// run context. Closures remain lexical continuations of their caller and are not isolated.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hermetic: bool,
}

impl InvocationFrame {
    /// the frame for a module's entry program.
    pub fn entry() -> Self {
        Self {
            function: None,
            body: None,
            ip: 0,
            stack: Vec::new(),
            locals: Vec::new(),
            awaiting: false,
            higher_order: None,
            hermetic: false,
        }
    }

    /// a frame for a named function call.
    pub fn for_function(name: impl Into<String>, locals: Vec<(String, Value)>) -> Self {
        Self {
            function: Some(name.into()),
            body: None,
            ip: 0,
            stack: Vec::new(),
            locals,
            awaiting: false,
            higher_order: None,
            hermetic: true,
        }
    }

    /// a frame for an applied closure, carrying the body it runs.
    pub fn for_closure(body: InvocationProgram, locals: Vec<(String, Value)>) -> Self {
        Self {
            function: None,
            body: Some(body),
            ip: 0,
            stack: Vec::new(),
            locals,
            awaiting: false,
            higher_order: None,
            hermetic: false,
        }
    }
}

/// Serializable state for a higher-order invocation while one of its lambda calls is running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HigherOrderState {
    pub name: String,
    pub closure: usize,
    pub items: Vec<Value>,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accumulator: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyed: Vec<(Value, Value)>,
}

/// a `Local` call's observed value, kept so a resume or replay does not observe it again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedLocal {
    pub sequence: i64,
    pub name: String,
    pub value: Value,
}

/// one closure: its parameters, its body, and the locals it captured where it was built.
///
/// capture is by value at construction, which is what makes it lexical — the closure sees the
/// bindings visible where it was written, not whatever happens to be in scope where it is applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosureCell {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    pub body: InvocationProgram,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captured: Vec<(String, Value)>,
}

/// the key a closure handle is stored under on the operand stack.
pub const CLOSURE_HANDLE_KEY: &str = "$closure";

/// build the operand-stack handle for a closure at `index`.
pub fn closure_handle(index: usize) -> Value {
    let mut map = crate::value::Map::new();
    map.insert(CLOSURE_HANDLE_KEY.to_string(), Value::from(index as i64));
    Value::Object(map)
}

/// read a closure handle's index back out of a value.
pub fn closure_handle_index(value: &Value) -> Option<usize> {
    value
        .get(CLOSURE_HANDLE_KEY)
        .and_then(|inner| inner.as_i64())
        .and_then(|index| usize::try_from(index).ok())
}
