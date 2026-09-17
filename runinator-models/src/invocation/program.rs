//! the stack-oriented program form and the continuation that freezes a run of it.

use super::*;

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

mod invocation_program;
pub use invocation_program::InvocationProgram;

mod call_policy;
pub use call_policy::CallPolicy;

mod call_retry;
pub use call_retry::CallRetry;

mod invocation_continuation;
pub use invocation_continuation::InvocationContinuation;

mod invocation_frame;
pub use invocation_frame::InvocationFrame;

mod higher_order_state;
pub use higher_order_state::HigherOrderState;

mod recorded_local;
pub use recorded_local::RecordedLocal;

mod closure_cell;
pub use closure_cell::ClosureCell;
