#[allow(unused_imports)]
use super::*;

/// One immutable execution-history record. `sequence` is per workflow run and is allocated by the
/// transaction that mutates the continuation/effect state, making UI history stable across retries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowJournalRecord {
    pub version: u32,
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    /// Derived from the journal entry by the backend rather than persisted independently.
    #[serde(default)]
    pub timeline_category: WorkflowTimelineCategory,
    pub entry: WorkflowJournalEntry,
    pub created_at: i64,
}

impl WorkflowJournalRecord {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_JOURNAL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_vm_version(
            WorkflowVmRecordKind::Journal,
            WORKFLOW_JOURNAL_VERSION,
            self.version,
        )
    }
}
