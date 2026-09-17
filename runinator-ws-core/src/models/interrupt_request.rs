#[allow(unused_imports)]
use super::*;

/// an interrupt asked for from outside the run. `source` defaults to `external`, which is the one
/// a caller normally has any business raising; the field exists so an operator can also drive the
/// other sources by hand. `continuation_id` names one thread of control in a fanned-out run, and
/// is omitted to let whichever real thread drives next take it.
#[derive(Debug, Deserialize)]
pub struct InterruptRequest {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub payload: Value,
    /// The thread to interrupt.
    #[serde(default)]
    pub continuation_id: Option<Uuid>,
}

impl Validate for InterruptRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("source", self.source.as_deref(), SHORT_TEXT_MAX)
    }
}
