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
pub use effect::*;
pub use program::*;

/// the ir version stamped on every module.
///
/// a stored continuation is only meaningful to the vm that produced it, so the version travels with
/// the module and a mismatch is an error rather than a best-effort decode.
pub const INVOCATION_IR_VERSION: u32 = 1;

/// a compiled unit: the entry program plus every function it can call by name.
///
/// user functions live here rather than in `metadata.functions` because they are *code*, and the
/// point of the ir is that there is one representation of code. the module is what a continuation
/// is resumed against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationModule {
    /// the ir version this module was compiled at; see [`INVOCATION_IR_VERSION`].
    pub version: u32,
    /// the program that runs when the invocation starts.
    pub entry: InvocationProgram,
    /// callable-by-name function bodies, keyed by the name the program calls them under.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<InvocationFunction>,
}

impl InvocationModule {
    /// a module holding a single program and no functions.
    pub fn new(entry: InvocationProgram) -> Self {
        Self {
            version: INVOCATION_IR_VERSION,
            entry,
            functions: Vec::new(),
        }
    }

    /// look up a function body by name.
    pub fn function(&self, name: &str) -> Option<&InvocationFunction> {
        self.functions.iter().find(|item| item.name == name)
    }

    /// whether this module's version is one the current vm understands.
    pub fn is_supported(&self) -> bool {
        self.version == INVOCATION_IR_VERSION
    }
}

/// one named function body plus the parameters it binds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationFunction {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    pub body: InvocationProgram,
    /// the annotated recursion cap from `@recursive(max_depth: N)`, when the author set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

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
