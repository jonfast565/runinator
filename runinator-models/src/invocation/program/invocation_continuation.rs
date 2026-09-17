#[allow(unused_imports)]
use super::*;

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
