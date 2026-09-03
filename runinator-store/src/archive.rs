use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use runinator_models::value::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ArchiveTable {
    WorkflowRuns,
    WorkflowVmModules,
    WorkflowContinuations,
    WorkflowEffects,
    WorkflowEffectOutputEvents,
    WorkflowEffectDispatches,
    WorkflowJournalEntries,
    WorkflowTriggerFirings,
    PipelineRuns,
    PipelineMemberAttempts,
    PipelineTriggerFirings,
    PipelineRevisions,
    Notifications,
    NotificationDeliveries,
    AutomationRecords,
    Gates,
    OrgUsageLedger,
    WorkflowRevisions,
    WorkflowFiles,
    IngressAdmissions,
    IngressEvents,
    OrchestrationBindings,
    OrchestrationEpochs,
    OrchestrationEventReductions,
    OrchestrationPendingIntents,
    OrchestrationCommands,
    OrchestrationEvidence,
    ExternalOperations,
    WorkspaceLeases,
    OrchestrationCorrelationAliases,
    AgentDirectives,
    DeadLetters,
    AuditLog,
    IdempotencyKeys,
}

impl ArchiveTable {
    pub const ALL: [ArchiveTable; 34] = [
        ArchiveTable::WorkflowEffectOutputEvents,
        ArchiveTable::WorkflowEffectDispatches,
        ArchiveTable::WorkflowEffects,
        ArchiveTable::WorkflowJournalEntries,
        ArchiveTable::WorkflowContinuations,
        ArchiveTable::WorkflowVmModules,
        ArchiveTable::WorkflowFiles,
        ArchiveTable::PipelineMemberAttempts,
        ArchiveTable::WorkflowRuns,
        ArchiveTable::WorkflowTriggerFirings,
        ArchiveTable::PipelineTriggerFirings,
        ArchiveTable::OrchestrationPendingIntents,
        ArchiveTable::OrchestrationCommands,
        ArchiveTable::OrchestrationEvidence,
        ArchiveTable::ExternalOperations,
        ArchiveTable::WorkspaceLeases,
        ArchiveTable::OrchestrationCorrelationAliases,
        ArchiveTable::OrchestrationEventReductions,
        ArchiveTable::OrchestrationEpochs,
        ArchiveTable::IngressEvents,
        ArchiveTable::OrchestrationBindings,
        ArchiveTable::IngressAdmissions,
        ArchiveTable::PipelineRuns,
        ArchiveTable::PipelineRevisions,
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
            ArchiveTable::WorkflowRuns => "workflow_runs",
            ArchiveTable::WorkflowVmModules => "workflow_vm_modules",
            ArchiveTable::WorkflowContinuations => "workflow_continuations",
            ArchiveTable::WorkflowEffects => "workflow_effects",
            ArchiveTable::WorkflowEffectOutputEvents => "workflow_effect_output_events",
            ArchiveTable::WorkflowEffectDispatches => "workflow_effect_dispatches",
            ArchiveTable::WorkflowJournalEntries => "workflow_journal_entries",
            ArchiveTable::WorkflowTriggerFirings => "workflow_trigger_firings",
            ArchiveTable::PipelineRuns => "pipeline_runs",
            ArchiveTable::PipelineMemberAttempts => "pipeline_member_attempts",
            ArchiveTable::PipelineTriggerFirings => "pipeline_trigger_firings",
            ArchiveTable::PipelineRevisions => "pipeline_revisions",
            ArchiveTable::Notifications => "notifications",
            ArchiveTable::NotificationDeliveries => "notification_deliveries",
            ArchiveTable::AutomationRecords => "automation_records",
            ArchiveTable::Gates => "gates",
            ArchiveTable::OrgUsageLedger => "org_usage_ledger",
            ArchiveTable::WorkflowRevisions => "workflow_revisions",
            ArchiveTable::WorkflowFiles => "workflow_files",
            ArchiveTable::IngressAdmissions => "ingress_admissions",
            ArchiveTable::IngressEvents => "ingress_events",
            ArchiveTable::OrchestrationBindings => "orchestration_bindings",
            ArchiveTable::OrchestrationEpochs => "orchestration_epochs",
            ArchiveTable::OrchestrationEventReductions => "orchestration_event_reductions",
            ArchiveTable::OrchestrationPendingIntents => "orchestration_pending_intents",
            ArchiveTable::OrchestrationCommands => "orchestration_commands",
            ArchiveTable::OrchestrationEvidence => "orchestration_evidence",
            ArchiveTable::ExternalOperations => "external_operations",
            ArchiveTable::WorkspaceLeases => "workspace_leases",
            ArchiveTable::OrchestrationCorrelationAliases => "orchestration_correlation_aliases",
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
            "workflow_vm_modules" => Ok(ArchiveTable::WorkflowVmModules),
            "workflow_continuations" => Ok(ArchiveTable::WorkflowContinuations),
            "workflow_effects" => Ok(ArchiveTable::WorkflowEffects),
            "workflow_effect_output_events" => Ok(ArchiveTable::WorkflowEffectOutputEvents),
            "workflow_effect_dispatches" => Ok(ArchiveTable::WorkflowEffectDispatches),
            "workflow_journal_entries" => Ok(ArchiveTable::WorkflowJournalEntries),
            "workflow_runs" => Ok(ArchiveTable::WorkflowRuns),
            "workflow_trigger_firings" => Ok(ArchiveTable::WorkflowTriggerFirings),
            "pipeline_runs" => Ok(ArchiveTable::PipelineRuns),
            "pipeline_member_attempts" => Ok(ArchiveTable::PipelineMemberAttempts),
            "pipeline_trigger_firings" => Ok(ArchiveTable::PipelineTriggerFirings),
            "pipeline_revisions" => Ok(ArchiveTable::PipelineRevisions),
            "notifications" => Ok(ArchiveTable::Notifications),
            "notification_deliveries" => Ok(ArchiveTable::NotificationDeliveries),
            "automation_records" => Ok(ArchiveTable::AutomationRecords),
            "gates" => Ok(ArchiveTable::Gates),
            "org_usage_ledger" => Ok(ArchiveTable::OrgUsageLedger),
            "workflow_revisions" => Ok(ArchiveTable::WorkflowRevisions),
            "workflow_files" => Ok(ArchiveTable::WorkflowFiles),
            "ingress_admissions" => Ok(ArchiveTable::IngressAdmissions),
            "ingress_events" => Ok(ArchiveTable::IngressEvents),
            "orchestration_bindings" => Ok(ArchiveTable::OrchestrationBindings),
            "orchestration_epochs" => Ok(ArchiveTable::OrchestrationEpochs),
            "orchestration_event_reductions" => Ok(ArchiveTable::OrchestrationEventReductions),
            "orchestration_pending_intents" => Ok(ArchiveTable::OrchestrationPendingIntents),
            "orchestration_commands" => Ok(ArchiveTable::OrchestrationCommands),
            "orchestration_evidence" => Ok(ArchiveTable::OrchestrationEvidence),
            "external_operations" => Ok(ArchiveTable::ExternalOperations),
            "workspace_leases" => Ok(ArchiveTable::WorkspaceLeases),
            "orchestration_correlation_aliases" => {
                Ok(ArchiveTable::OrchestrationCorrelationAliases)
            }
            "agent_directives" => Ok(ArchiveTable::AgentDirectives),
            "dead_letters" => Ok(ArchiveTable::DeadLetters),
            "audit_log" => Ok(ArchiveTable::AuditLog),
            "idempotency_keys" => Ok(ArchiveTable::IdempotencyKeys),
            other => Err(format!("unsupported archive table '{other}'")),
        }
    }
}

/// The durable lifecycle assigned to every application table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableDataPolicy {
    /// Rows are copied to compressed cold storage before deletion.
    ColdArchive,
    /// Rows are deleted by a foreign-key cascade with a parent that has its own policy.
    CascadeWithParent,
    /// A bounded service loop or reference-aware garbage collector prunes the table.
    ServiceRetention,
    /// The table is bounded mutable state rather than append-only history.
    BoundedState,
    /// Operators or resource APIs own creation and explicit deletion.
    ExplicitLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseTablePolicy {
    pub table: &'static str,
    pub policy: TableDataPolicy,
}

macro_rules! table_policy {
    ($table:literal, $policy:ident) => {
        DatabaseTablePolicy {
            table: $table,
            policy: TableDataPolicy::$policy,
        }
    };
}

/// Exhaustive schema policy inventory. The database suite compares this list with the migrated
/// schema so adding a table without choosing its lifecycle is a test failure.
pub const DATABASE_TABLE_POLICIES: &[DatabaseTablePolicy] = &[
    table_policy!("agent_directives", ColdArchive),
    table_policy!("agent_enrollment_tokens", ServiceRetention),
    table_policy!("api_keys", ExplicitLifecycle),
    table_policy!("archive_marks", ServiceRetention),
    table_policy!("audit_log", ColdArchive),
    table_policy!("auth_sessions", ServiceRetention),
    table_policy!("automation_records", ColdArchive),
    table_policy!("broker_messages", ServiceRetention),
    table_policy!("broker_ingress_messages", ServiceRetention),
    table_policy!("broker_ingress_sessions", BoundedState),
    table_policy!("calendar_subscriptions", ExplicitLifecycle),
    table_policy!("catalog_items", ExplicitLifecycle),
    table_policy!("console_bindings", CascadeWithParent),
    table_policy!("console_cells", CascadeWithParent),
    table_policy!("console_functions", CascadeWithParent),
    table_policy!("console_sessions", ExplicitLifecycle),
    table_policy!("dead_letters", ColdArchive),
    table_policy!("execution_profile_revisions", CascadeWithParent),
    table_policy!("execution_profiles", ExplicitLifecycle),
    table_policy!("external_operations", ColdArchive),
    table_policy!("ingress_control_events", ServiceRetention),
    table_policy!("ingress_control_gates", BoundedState),
    table_policy!("freeze_windows", ExplicitLifecycle),
    table_policy!("function_adapter_workflows", CascadeWithParent),
    table_policy!("function_aliases", CascadeWithParent),
    table_policy!("function_artifacts", ServiceRetention),
    table_policy!("function_exports", CascadeWithParent),
    table_policy!("function_packages", ExplicitLifecycle),
    table_policy!("function_versions", ExplicitLifecycle),
    table_policy!("gates", ColdArchive),
    table_policy!("idempotency_keys", ColdArchive),
    table_policy!("ingress_admissions", ColdArchive),
    table_policy!("ingress_events", ColdArchive),
    table_policy!("notification_deliveries", ColdArchive),
    table_policy!("notification_policies", ExplicitLifecycle),
    table_policy!("notifications", ColdArchive),
    table_policy!("orchestration_adapter_polls", BoundedState),
    table_policy!("orchestration_adapter_revisions", ExplicitLifecycle),
    table_policy!("orchestration_adapters", ExplicitLifecycle),
    table_policy!("orchestration_bindings", ColdArchive),
    table_policy!("orchestration_commands", ColdArchive),
    table_policy!("orchestration_correlation_aliases", ColdArchive),
    table_policy!("orchestration_epochs", ColdArchive),
    table_policy!("orchestration_event_reductions", ColdArchive),
    table_policy!("orchestration_evidence", ColdArchive),
    table_policy!("orchestration_pending_intents", ColdArchive),
    table_policy!("org_resource_groups", BoundedState),
    table_policy!("org_usage_ledger", ColdArchive),
    table_policy!("organizations", ExplicitLifecycle),
    table_policy!("pipeline_member_attempts", ColdArchive),
    table_policy!("pipeline_revisions", ColdArchive),
    table_policy!("pipeline_runs", ColdArchive),
    table_policy!("pipeline_trigger_firings", ColdArchive),
    table_policy!("pipeline_triggers", ExplicitLifecycle),
    table_policy!("pipelines", ExplicitLifecycle),
    table_policy!("replica_provider_registrations", CascadeWithParent),
    table_policy!("replica_samples", ServiceRetention),
    table_policy!("replicas", ServiceRetention),
    table_policy!("resource_grants", ExplicitLifecycle),
    table_policy!("resource_ownership", BoundedState),
    table_policy!("role_assignments", ExplicitLifecycle),
    table_policy!("service_accounts", ExplicitLifecycle),
    table_policy!("settings", BoundedState),
    table_policy!("teams", ExplicitLifecycle),
    table_policy!("user_identities", CascadeWithParent),
    table_policy!("users", ExplicitLifecycle),
    table_policy!("workflow_continuations", ColdArchive),
    table_policy!("workflow_cooldowns", ServiceRetention),
    table_policy!("workflow_cursor_frames", CascadeWithParent),
    table_policy!("workflow_effect_dispatches", ColdArchive),
    table_policy!("workflow_effect_output_events", ColdArchive),
    table_policy!("workflow_effects", ColdArchive),
    table_policy!("workflow_files", ColdArchive),
    table_policy!("workflow_journal_entries", ColdArchive),
    table_policy!("workflow_mutexes", ServiceRetention),
    table_policy!("workflow_revisions", ColdArchive),
    table_policy!("workflow_run_cursors", CascadeWithParent),
    table_policy!("workflow_run_event_sources", CascadeWithParent),
    table_policy!("workflow_run_frames", CascadeWithParent),
    table_policy!("workflow_run_pending_interrupts", CascadeWithParent),
    table_policy!("workflow_runs", ColdArchive),
    table_policy!("workflow_timer_interrupts", CascadeWithParent),
    table_policy!("workflow_trigger_firings", ColdArchive),
    table_policy!("workflow_triggers", ExplicitLifecycle),
    table_policy!("workflow_vm_modules", ColdArchive),
    table_policy!("workflows", ExplicitLifecycle),
    table_policy!("workspace_leases", ColdArchive),
];

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
