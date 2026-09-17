//! the versioned invocation ir: one program form for every callable, and the continuation that
//! lets a half-finished program be persisted and resumed.
//!
//! this replaces the split between "an expression the reducer folds" and "a compute program the
//! worker interprets". a single [`InvocationProgram`] covers both: the vm runs it until it either
//! finishes or reaches a call it cannot make in process, at which point it hands back an
//! [`InvocationEffect`] plus the [`InvocationContinuation`] needed to pick up where it stopped.
//!
//! the types live here, in the lowest shared crate, because they cross every boundary that matters:
//! the compiler writes them, the reducer steps them, the store persists them, and the broker
//! carries the effects they yield. evaluation itself stays in `runinator-compute`.

use serde::{Deserialize, Serialize};

use crate::functions::FunctionBinding;
use crate::value::Value;

mod effect;
mod program;
mod record;
pub use effect::*;
pub use program::*;
pub use record::*;

/// the ir version stamped on every module.
///
/// a stored continuation is only meaningful to the vm that produced it, so the version travels with
/// the module and a mismatch is an error rather than a best-effort decode.
pub const INVOCATION_IR_VERSION: u32 = 1;

/// what one `step`/`resume` of the vm produced.
///
/// `Goto` is a first-class outcome rather than a kind of completion because a `goto` moves the
/// *cursor*, which is the run's business and not the vm's — the vm reports the jump and the reducer
/// decides what it means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvocationStep {
    /// the program ran to completion with this value.
    Complete { value: Value },
    /// the program reached a call it cannot make in process. the effect describes the call; the
    /// continuation is what to resume once the call has a result.
    Yield {
        effect: Box<InvocationEffect>,
        continuation: Box<InvocationContinuation>,
    },
    /// the program raised an error.
    Failed { message: String },
    /// the program executed `goto <target>`, moving this thread of control.
    Goto { target: String },
}

impl InvocationStep {
    /// whether this step ended the program.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Yield { .. })
    }
}

#[cfg(test)]
#[path = "invocation/tests.rs"]
mod tests;

mod invocation_module;
pub use invocation_module::InvocationModule;

mod invocation_function;
pub use invocation_function::InvocationFunction;
