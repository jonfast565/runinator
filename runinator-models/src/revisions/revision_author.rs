#[allow(unused_imports)]
use super::*;

/// who is writing a definition, and why. passed into the engine's save path so a revision can be
/// attributed without the engine depending on the web service's auth extraction — the same shape
/// `record_audit` uses for the audit trail.
#[derive(Debug, Clone, Default)]
pub struct RevisionAuthor {
    /// Set only after the transport establishes workflow Own or scoped Own authority.
    pub contract_override_reason: Option<String>,
    pub actor_id: Option<Uuid>,
    pub actor_kind: String,
    pub source: RevisionSource,
    pub note: Option<String>,
}

impl RevisionAuthor {
    /// an unattributed write performed by the platform itself (background reconcile, tests).
    pub fn system(source: RevisionSource) -> Self {
        Self {
            contract_override_reason: None,
            actor_id: None,
            actor_kind: "system".to_string(),
            source,
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}
