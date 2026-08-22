use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use runinator_models::value::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ArchiveTable {
    Runs,
    RunArtifacts,
    WorkflowRuns,
    WorkflowVmModules,
    WorkflowContinuations,
    WorkflowEffects,
    WorkflowEffectOutputEvents,
    WorkflowEffectDispatches,
    WorkflowJournalEntries,
    WorkflowTriggerFirings,
    RunChunks,
    PipelineRuns,
    PipelineTriggerFirings,
    Notifications,
    NotificationDeliveries,
    AutomationRecords,
    Gates,
    OrgUsageLedger,
    WorkflowRevisions,
    AgentDirectives,
    DeadLetters,
    AuditLog,
    IdempotencyKeys,
}

impl ArchiveTable {
    pub const ALL: [ArchiveTable; 23] = [
        ArchiveTable::RunArtifacts,
        ArchiveTable::RunChunks,
        ArchiveTable::Runs,
        ArchiveTable::WorkflowEffectOutputEvents,
        ArchiveTable::WorkflowEffectDispatches,
        ArchiveTable::WorkflowEffects,
        ArchiveTable::WorkflowJournalEntries,
        ArchiveTable::WorkflowContinuations,
        ArchiveTable::WorkflowVmModules,
        ArchiveTable::WorkflowRuns,
        ArchiveTable::WorkflowTriggerFirings,
        ArchiveTable::PipelineTriggerFirings,
        ArchiveTable::PipelineRuns,
        ArchiveTable::NotificationDeliveries,
        ArchiveTable::Notifications,
        ArchiveTable::AutomationRecords,
        ArchiveTable::Gates,
        ArchiveTable::OrgUsageLedger,
        ArchiveTable::WorkflowRevisions,
        ArchiveTable::AgentDirectives,
        ArchiveTable::DeadLetters,
        ArchiveTable::AuditLog,
        ArchiveTable::IdempotencyKeys,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveTable::Runs => "runs",
            ArchiveTable::RunArtifacts => "run_artifacts",
            ArchiveTable::WorkflowRuns => "workflow_runs",
            ArchiveTable::WorkflowVmModules => "workflow_vm_modules",
            ArchiveTable::WorkflowContinuations => "workflow_continuations",
            ArchiveTable::WorkflowEffects => "workflow_effects",
            ArchiveTable::WorkflowEffectOutputEvents => "workflow_effect_output_events",
            ArchiveTable::WorkflowEffectDispatches => "workflow_effect_dispatches",
            ArchiveTable::WorkflowJournalEntries => "workflow_journal_entries",
            ArchiveTable::WorkflowTriggerFirings => "workflow_trigger_firings",
            ArchiveTable::RunChunks => "run_chunks",
            ArchiveTable::PipelineRuns => "pipeline_runs",
            ArchiveTable::PipelineTriggerFirings => "pipeline_trigger_firings",
            ArchiveTable::Notifications => "notifications",
            ArchiveTable::NotificationDeliveries => "notification_deliveries",
            ArchiveTable::AutomationRecords => "automation_records",
            ArchiveTable::Gates => "gates",
            ArchiveTable::OrgUsageLedger => "org_usage_ledger",
            ArchiveTable::WorkflowRevisions => "workflow_revisions",
            ArchiveTable::AgentDirectives => "agent_directives",
            ArchiveTable::DeadLetters => "dead_letters",
            ArchiveTable::AuditLog => "audit_log",
            ArchiveTable::IdempotencyKeys => "idempotency_keys",
        }
    }

    pub fn primary_key_column(self) -> &'static str {
        match self {
            ArchiveTable::WorkflowEffectOutputEvents => "event_id",
            ArchiveTable::AgentDirectives => "directive_id",
            _ => "id",
        }
    }
}

impl fmt::Display for ArchiveTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ArchiveTable {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "runs" => Ok(ArchiveTable::Runs),
            "run_artifacts" => Ok(ArchiveTable::RunArtifacts),
            "workflow_vm_modules" => Ok(ArchiveTable::WorkflowVmModules),
            "workflow_continuations" => Ok(ArchiveTable::WorkflowContinuations),
            "workflow_effects" => Ok(ArchiveTable::WorkflowEffects),
            "workflow_effect_output_events" => Ok(ArchiveTable::WorkflowEffectOutputEvents),
            "workflow_effect_dispatches" => Ok(ArchiveTable::WorkflowEffectDispatches),
            "workflow_journal_entries" => Ok(ArchiveTable::WorkflowJournalEntries),
            "workflow_runs" => Ok(ArchiveTable::WorkflowRuns),
            "workflow_trigger_firings" => Ok(ArchiveTable::WorkflowTriggerFirings),
            "run_chunks" => Ok(ArchiveTable::RunChunks),
            "pipeline_runs" => Ok(ArchiveTable::PipelineRuns),
            "pipeline_trigger_firings" => Ok(ArchiveTable::PipelineTriggerFirings),
            "notifications" => Ok(ArchiveTable::Notifications),
            "notification_deliveries" => Ok(ArchiveTable::NotificationDeliveries),
            "automation_records" => Ok(ArchiveTable::AutomationRecords),
            "gates" => Ok(ArchiveTable::Gates),
            "org_usage_ledger" => Ok(ArchiveTable::OrgUsageLedger),
            "workflow_revisions" => Ok(ArchiveTable::WorkflowRevisions),
            "agent_directives" => Ok(ArchiveTable::AgentDirectives),
            "dead_letters" => Ok(ArchiveTable::DeadLetters),
            "audit_log" => Ok(ArchiveTable::AuditLog),
            "idempotency_keys" => Ok(ArchiveTable::IdempotencyKeys),
            other => Err(format!("unsupported archive table '{other}'")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ArchiveMark {
    pub id: Uuid,
    pub table: ArchiveTable,
    pub primary_key: Uuid,
    pub created_at: DateTime<Utc>,
    pub eligible_before: DateTime<Utc>,
    pub archive_day: String,
}

#[derive(Clone, Debug)]
pub struct ArchiveRow {
    pub mark_id: Uuid,
    pub table: ArchiveTable,
    pub primary_key: Uuid,
    pub created_at: DateTime<Utc>,
    pub row: Value,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::ArchiveTable;

    #[test]
    fn archive_tables_round_trip_through_the_persisted_table_name() {
        for table in ArchiveTable::ALL {
            assert_eq!(ArchiveTable::from_str(table.as_str()), Ok(table));
        }
    }

    #[test]
    fn only_effect_output_events_use_an_event_id_primary_key() {
        assert_eq!(
            ArchiveTable::WorkflowEffectOutputEvents.primary_key_column(),
            "event_id"
        );
        assert_eq!(
            ArchiveTable::WorkflowJournalEntries.primary_key_column(),
            "id"
        );
    }
}
