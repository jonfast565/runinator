#[allow(unused_imports)]
use super::*;

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
