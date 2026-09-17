#[allow(unused_imports)]
use super::*;

#[allow(dead_code)]
pub(crate) trait ArchiveTableSql {
    fn archive_candidate_sql(self) -> &'static str;
    fn archive_source_sql(self, dialect: SqlDialect) -> String;
    fn archive_source_sql_v2(self, dialect: SqlDialect) -> String;
    fn archive_columns(self) -> &'static [ArchiveColumn];
    fn archive_source_predicate(self) -> &'static str;
    fn archive_row_json<R>(self, row: &R) -> Result<Value, SendableError>
    where
        R: Row,
        for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> bool: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<i64>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<String>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<Uuid>: Decode<'r, R::Database> + Type<R::Database>,
        for<'c> &'c str: ColumnIndex<R>;
    fn archive_row_json_v2<R>(self, row: &R) -> Result<Value, SendableError>
    where
        R: Row,
        for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> bool: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<i64>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<String>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<Uuid>: Decode<'r, R::Database> + Type<R::Database>,
        for<'c> &'c str: ColumnIndex<R>;
}

impl ArchiveTableSql for ArchiveTable {
    fn archive_candidate_sql(self) -> &'static str {
        match self {
        ArchiveTable::WorkflowRuns => {
            "SELECT id, created_at FROM workflow_runs
             WHERE created_at <= ?
               AND status IN ('succeeded', 'failed', 'timed_out', 'canceled')
               AND NOT EXISTS (SELECT 1 FROM workflow_vm_modules WHERE workflow_vm_modules.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_continuations WHERE workflow_continuations.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_effects JOIN workflow_continuations ON workflow_continuations.id = workflow_effects.continuation_id WHERE workflow_continuations.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_journal_entries WHERE workflow_journal_entries.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_ai_usage WHERE workflow_ai_usage.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_trigger_firings WHERE workflow_trigger_firings.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM automation_records WHERE automation_records.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM gates WHERE gates.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_files WHERE workflow_files.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM pipeline_member_attempts WHERE pipeline_member_attempts.workflow_run_id = workflow_runs.id)
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::WorkflowVmModules => {
            "SELECT workflow_vm_modules.workflow_run_id AS id, workflow_vm_modules.created_at FROM workflow_vm_modules
             WHERE workflow_vm_modules.created_at <= ?
               AND EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_vm_modules.workflow_run_id
                 AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))
               AND NOT EXISTS (SELECT 1 FROM workflow_continuations WHERE workflow_continuations.workflow_run_id = workflow_vm_modules.workflow_run_id)
               AND NOT EXISTS (SELECT 1 FROM workflow_effects JOIN workflow_continuations ON workflow_continuations.id = workflow_effects.continuation_id WHERE workflow_continuations.workflow_run_id = workflow_vm_modules.workflow_run_id)
               AND NOT EXISTS (SELECT 1 FROM workflow_journal_entries WHERE workflow_journal_entries.workflow_run_id = workflow_vm_modules.workflow_run_id)
             ORDER BY workflow_vm_modules.created_at, workflow_vm_modules.workflow_run_id LIMIT ?"
        }
        ArchiveTable::WorkflowContinuations => {
            "SELECT workflow_continuations.id, workflow_continuations.created_at FROM workflow_continuations
             WHERE workflow_continuations.created_at <= ?
               AND EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_continuations.workflow_run_id
                 AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))
               AND NOT EXISTS (SELECT 1 FROM workflow_effects WHERE workflow_effects.continuation_id = workflow_continuations.id)
             ORDER BY workflow_continuations.created_at, workflow_continuations.id LIMIT ?"
        }
        ArchiveTable::WorkflowEffects => {
            "SELECT workflow_effects.id, workflow_effects.created_at FROM workflow_effects
             WHERE workflow_effects.created_at <= ?
               AND status IN ('succeeded', 'failed', 'timed_out', 'canceled')
               AND NOT EXISTS (SELECT 1 FROM workflow_effect_output_events WHERE workflow_effect_output_events.effect_id = workflow_effects.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_ai_usage WHERE workflow_ai_usage.effect_id = workflow_effects.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_effect_dispatches WHERE workflow_effect_dispatches.effect_id = workflow_effects.id)
             ORDER BY workflow_effects.created_at, workflow_effects.id LIMIT ?"
        }
        ArchiveTable::WorkflowEffectOutputEvents => {
            "SELECT workflow_effect_output_events.event_id AS id, workflow_effect_output_events.created_at FROM workflow_effect_output_events
             WHERE workflow_effect_output_events.created_at <= ?
               AND EXISTS (SELECT 1 FROM workflow_effects e JOIN workflow_continuations c ON c.id = e.continuation_id JOIN workflow_runs r ON r.id = c.workflow_run_id WHERE e.id = workflow_effect_output_events.effect_id
                 AND r.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))
             ORDER BY workflow_effect_output_events.created_at, workflow_effect_output_events.event_id LIMIT ?"
        }
        ArchiveTable::WorkflowAiUsage => {
            "SELECT workflow_ai_usage.event_id AS id, workflow_ai_usage.recorded_at AS created_at FROM workflow_ai_usage
             WHERE workflow_ai_usage.recorded_at <= ?
               AND EXISTS (SELECT 1 FROM workflow_runs r WHERE r.id = workflow_ai_usage.workflow_run_id
                 AND r.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))
             ORDER BY workflow_ai_usage.recorded_at, workflow_ai_usage.event_id LIMIT ?"
        }
        ArchiveTable::WorkflowEffectDispatches => {
            "SELECT workflow_effect_dispatches.id, workflow_effect_dispatches.created_at FROM workflow_effect_dispatches
             WHERE workflow_effect_dispatches.updated_at <= ?
               AND (published_at IS NOT NULL OR (attempts > 0 AND last_error IS NOT NULL))
             ORDER BY workflow_effect_dispatches.updated_at, workflow_effect_dispatches.id LIMIT ?"
        }
        ArchiveTable::WorkflowJournalEntries => {
            "SELECT workflow_journal_entries.id, workflow_journal_entries.created_at FROM workflow_journal_entries
             WHERE workflow_journal_entries.created_at <= ?
               AND EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_journal_entries.workflow_run_id
                 AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))
             ORDER BY workflow_journal_entries.created_at, workflow_journal_entries.id LIMIT ?"
        }
        ArchiveTable::WorkflowTriggerFirings => {
            "SELECT workflow_trigger_firings.id, workflow_trigger_firings.created_at FROM workflow_trigger_firings
             WHERE workflow_trigger_firings.created_at <= ? AND (workflow_run_id IS NULL OR EXISTS (
               SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_trigger_firings.workflow_run_id
                 AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')))
             ORDER BY workflow_trigger_firings.created_at, workflow_trigger_firings.id LIMIT ?"
        }
        ArchiveTable::PipelineRuns => {
            "SELECT id, created_at FROM pipeline_runs
             WHERE created_at <= ? AND status IN ('succeeded', 'failed', 'timed_out', 'canceled')
               AND NOT EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.pipeline_run_id = pipeline_runs.id)
               AND NOT EXISTS (SELECT 1 FROM pipeline_trigger_firings WHERE pipeline_trigger_firings.pipeline_run_id = pipeline_runs.id)
               AND NOT EXISTS (SELECT 1 FROM pipeline_member_attempts WHERE pipeline_member_attempts.pipeline_run_id = pipeline_runs.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_epochs WHERE orchestration_epochs.pipeline_run_id = pipeline_runs.id)
             ORDER BY created_at, id LIMIT ?"
        }
        ArchiveTable::PipelineMemberAttempts => {
            "SELECT pipeline_member_attempts.id, pipeline_member_attempts.created_at FROM pipeline_member_attempts
             WHERE pipeline_member_attempts.created_at <= ? AND EXISTS (
               SELECT 1 FROM pipeline_runs WHERE pipeline_runs.id = pipeline_member_attempts.pipeline_run_id
                 AND pipeline_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))
             ORDER BY pipeline_member_attempts.created_at, pipeline_member_attempts.id LIMIT ?"
        }
        ArchiveTable::PipelineTriggerFirings => {
            "SELECT pipeline_trigger_firings.id, pipeline_trigger_firings.created_at FROM pipeline_trigger_firings
             WHERE pipeline_trigger_firings.created_at <= ? AND (pipeline_run_id IS NULL OR EXISTS (
               SELECT 1 FROM pipeline_runs WHERE pipeline_runs.id = pipeline_trigger_firings.pipeline_run_id
                 AND pipeline_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')))
             ORDER BY pipeline_trigger_firings.created_at, pipeline_trigger_firings.id LIMIT ?"
        }
        ArchiveTable::PipelineRevisions => {
            "SELECT pipeline_revisions.id, pipeline_revisions.created_at FROM pipeline_revisions
             WHERE pipeline_revisions.created_at <= ? AND pipeline_revisions.revision < (
               SELECT MAX(newer.revision) FROM pipeline_revisions newer WHERE newer.pipeline_id = pipeline_revisions.pipeline_id)
             ORDER BY pipeline_revisions.created_at, pipeline_revisions.id LIMIT ?"
        }
        ArchiveTable::Notifications => {
            "SELECT id, created_at FROM notifications
             WHERE created_at <= ?
               AND NOT EXISTS (SELECT 1 FROM notification_deliveries WHERE notification_deliveries.notification_id = notifications.id)
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::NotificationDeliveries => {
            "SELECT notification_deliveries.id, notification_deliveries.created_at FROM notification_deliveries
             WHERE notification_deliveries.created_at <= ? AND notification_deliveries.status NOT IN ('pending', 'retrying')
             ORDER BY notification_deliveries.created_at, notification_deliveries.id LIMIT ?"
        }
        ArchiveTable::AutomationRecords => {
            "SELECT automation_records.id, automation_records.created_at FROM automation_records
             WHERE automation_records.created_at <= ? AND (automation_records.resolved_at IS NOT NULL OR EXISTS (
               SELECT 1 FROM workflow_runs WHERE workflow_runs.id = automation_records.workflow_run_id
                 AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')))
             ORDER BY automation_records.created_at, automation_records.id LIMIT ?"
        }
        ArchiveTable::Gates => {
            "SELECT gates.id, gates.created_at FROM gates
             WHERE gates.created_at <= ? AND (gates.resolved_at IS NOT NULL OR EXISTS (
               SELECT 1 FROM workflow_runs WHERE workflow_runs.id = gates.workflow_run_id
                 AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')))
             ORDER BY gates.created_at, gates.id LIMIT ?"
        }
        ArchiveTable::OrgUsageLedger => {
            "SELECT id, sampled_at AS created_at FROM org_usage_ledger WHERE sampled_at <= ? ORDER BY sampled_at, id LIMIT ?"
        }
        ArchiveTable::WorkflowRevisions => {
            "SELECT workflow_revisions.id, workflow_revisions.created_at FROM workflow_revisions
             WHERE workflow_revisions.created_at <= ? AND workflow_revisions.revision < (
               SELECT MAX(newer.revision) FROM workflow_revisions newer WHERE newer.workflow_id = workflow_revisions.workflow_id)
             ORDER BY workflow_revisions.created_at, workflow_revisions.id LIMIT ?"
        }
        ArchiveTable::WorkflowFiles => {
            "SELECT workflow_files.id, workflow_files.created_at FROM workflow_files
             WHERE workflow_files.created_at <= ? AND (
               (workflow_files.scope = 'run' AND EXISTS (
                 SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_files.workflow_run_id
                   AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')))
               OR (workflow_files.scope = 'library' AND (workflow_files.archived = TRUE OR workflow_files.is_current = FALSE))
               OR workflow_files.scope = 'staged')
             ORDER BY workflow_files.created_at, workflow_files.id LIMIT ?"
        }
        ArchiveTable::OrchestrationPendingIntents => {
            "SELECT orchestration_pending_intents.id, orchestration_pending_intents.created_at FROM orchestration_pending_intents
             WHERE orchestration_pending_intents.created_at <= ? AND EXISTS (
               SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_pending_intents.binding_id
                 AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY orchestration_pending_intents.created_at, orchestration_pending_intents.id LIMIT ?"
        }
        ArchiveTable::OrchestrationCommands => {
            "SELECT orchestration_commands.id, orchestration_commands.created_at FROM orchestration_commands
             WHERE orchestration_commands.created_at <= ?
               AND orchestration_commands.status IN ('succeeded', 'failed', 'superseded')
               AND EXISTS (SELECT 1 FROM orchestration_bindings
                 WHERE orchestration_bindings.id = orchestration_commands.binding_id
                   AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY orchestration_commands.created_at, orchestration_commands.id LIMIT ?"
        }
        ArchiveTable::OrchestrationEvidence => {
            "SELECT orchestration_evidence.id, orchestration_evidence.created_at FROM orchestration_evidence
             WHERE orchestration_evidence.created_at <= ? AND EXISTS (
               SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_evidence.binding_id
                 AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY orchestration_evidence.created_at, orchestration_evidence.id LIMIT ?"
        }
        ArchiveTable::ExternalOperations => {
            "SELECT external_operations.id, external_operations.created_at FROM external_operations
             WHERE external_operations.created_at <= ?
               AND external_operations.status IN ('succeeded', 'failed')
               AND EXISTS (SELECT 1 FROM orchestration_bindings
                 WHERE orchestration_bindings.id = external_operations.binding_id
                   AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY external_operations.created_at, external_operations.id LIMIT ?"
        }
        ArchiveTable::WorkspaceLeases => {
            "SELECT workspace_leases.id, workspace_leases.created_at FROM workspace_leases
             WHERE workspace_leases.created_at <= ? AND workspace_leases.status IN ('released', 'abandoned')
               AND EXISTS (SELECT 1 FROM ingress_admissions
                 WHERE ingress_admissions.id = workspace_leases.admission_id
                   AND ingress_admissions.status = 'terminal')
             ORDER BY workspace_leases.created_at, workspace_leases.id LIMIT ?"
        }
        ArchiveTable::OrchestrationCorrelationAliases => {
            "SELECT orchestration_correlation_aliases.id, orchestration_correlation_aliases.created_at FROM orchestration_correlation_aliases
             WHERE orchestration_correlation_aliases.created_at <= ? AND EXISTS (
               SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_correlation_aliases.binding_id
                 AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY orchestration_correlation_aliases.created_at, orchestration_correlation_aliases.id LIMIT ?"
        }
        ArchiveTable::OrchestrationEventReductions => {
            "SELECT orchestration_event_reductions.id, orchestration_event_reductions.created_at FROM orchestration_event_reductions
             WHERE orchestration_event_reductions.created_at <= ? AND EXISTS (
               SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_event_reductions.binding_id
                 AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY orchestration_event_reductions.created_at, orchestration_event_reductions.id LIMIT ?"
        }
        ArchiveTable::OrchestrationEpochs => {
            "SELECT orchestration_epochs.id, orchestration_epochs.created_at FROM orchestration_epochs
             WHERE orchestration_epochs.created_at <= ? AND EXISTS (
               SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_epochs.binding_id
                 AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))
             ORDER BY orchestration_epochs.created_at, orchestration_epochs.id LIMIT ?"
        }
        ArchiveTable::IngressEvents => {
            "SELECT ingress_events.id, ingress_events.received_at AS created_at FROM ingress_events
             WHERE ingress_events.received_at <= ? AND EXISTS (
               SELECT 1 FROM ingress_admissions WHERE ingress_admissions.id = ingress_events.admission_id
                 AND ingress_admissions.status = 'terminal')
               AND NOT EXISTS (SELECT 1 FROM orchestration_event_reductions
                 WHERE orchestration_event_reductions.inbox_event_id = ingress_events.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_evidence
                 WHERE orchestration_evidence.source_event_id = ingress_events.id)
             ORDER BY ingress_events.received_at, ingress_events.id LIMIT ?"
        }
        ArchiveTable::OrchestrationBindings => {
            "SELECT orchestration_bindings.id, orchestration_bindings.created_at FROM orchestration_bindings
             WHERE orchestration_bindings.created_at <= ?
               AND orchestration_bindings.status IN ('completed', 'failed', 'terminated')
               AND NOT EXISTS (SELECT 1 FROM orchestration_epochs WHERE orchestration_epochs.binding_id = orchestration_bindings.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_event_reductions WHERE orchestration_event_reductions.binding_id = orchestration_bindings.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_pending_intents WHERE orchestration_pending_intents.binding_id = orchestration_bindings.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_commands WHERE orchestration_commands.binding_id = orchestration_bindings.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_evidence WHERE orchestration_evidence.binding_id = orchestration_bindings.id)
               AND NOT EXISTS (SELECT 1 FROM external_operations WHERE external_operations.binding_id = orchestration_bindings.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_correlation_aliases WHERE orchestration_correlation_aliases.binding_id = orchestration_bindings.id)
             ORDER BY orchestration_bindings.created_at, orchestration_bindings.id LIMIT ?"
        }
        ArchiveTable::IngressAdmissions => {
            "SELECT ingress_admissions.id, ingress_admissions.created_at FROM ingress_admissions
             WHERE ingress_admissions.created_at <= ? AND ingress_admissions.status = 'terminal'
               AND NOT EXISTS (SELECT 1 FROM ingress_events WHERE ingress_events.admission_id = ingress_admissions.id)
               AND NOT EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.admission_id = ingress_admissions.id)
               AND NOT EXISTS (SELECT 1 FROM workspace_leases WHERE workspace_leases.admission_id = ingress_admissions.id)
             ORDER BY ingress_admissions.created_at, ingress_admissions.id LIMIT ?"
        }
        ArchiveTable::AgentDirectives => {
            "SELECT directive_id AS id, issued_at AS created_at FROM agent_directives
             WHERE issued_at <= ? AND completed_at IS NOT NULL AND state IN ('completed', 'failed', 'unsupported', 'expired')
             ORDER BY issued_at, directive_id LIMIT ?"
        }
        ArchiveTable::DeadLetters => {
            "SELECT id, created_at FROM dead_letters
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::AuditLog => {
            "SELECT id, created_at FROM audit_log
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::IdempotencyKeys => {
            "SELECT id, created_at FROM idempotency_keys
             WHERE created_at <= ? AND (completed_at IS NOT NULL OR owner_node_run_id IS NULL)
             ORDER BY created_at, id
             LIMIT ?"
        }
    }
    }

    fn archive_source_sql_v2(self, dialect: SqlDialect) -> String {
        let columns = self
            .archive_columns()
            .iter()
            .map(|column| dialect.ident(column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let predicate = self.archive_source_predicate();
        let eligibility = if predicate.is_empty() {
            String::new()
        } else {
            format!(" AND ({predicate})")
        };
        format!(
            "SELECT {columns} FROM {table} WHERE {primary_key} = ?{eligibility}",
            table = self.as_str(),
            primary_key = dialect.ident(self.primary_key_column()),
        )
    }

    fn archive_columns(self) -> &'static [ArchiveColumn] {
        match self {
            ArchiveTable::WorkflowRuns => archive_columns![
                "id" => Uuid, "workflow_id" => Uuid, "workflow_snapshot" => OptionalText,
                "status" => Text, "active_node_id" => OptionalText, "parameters" => Text,
                "watch_fired" => Boolean, "run_metadata_json" => OptionalText,
                "extra_json" => Text, "created_at" => Integer, "started_at" => OptionalInteger,
                "finished_at" => OptionalInteger, "message" => OptionalText,
                "name" => OptionalText, "scheduler_claimed_by" => OptionalText,
                "scheduler_claimed_until" => OptionalInteger, "orchestration_version" => Integer,
                "trigger_source_kind" => OptionalText, "trigger_actor_type" => OptionalText,
                "trigger_actor_replica_id" => OptionalUuid,
                "trigger_actor_display_name" => OptionalText,
                "trigger_request_host" => OptionalText, "trigger_request_ip" => OptionalText,
                "trigger_metadata" => Text, "pipeline_run_id" => OptionalUuid,
                "correlation_key" => OptionalText, "state_version" => Integer,
            ],
            ArchiveTable::WorkflowVmModules => archive_columns![
                "workflow_run_id" => Uuid, "version" => Integer, "module_json" => Text,
                "created_at" => Integer,
            ],
            ArchiveTable::WorkflowContinuations => archive_columns![
                "id" => Uuid, "workflow_run_id" => Uuid, "module_version" => Integer,
                "continuation_json" => Text, "status" => Text, "version" => Integer,
                "ready_at" => OptionalInteger, "claimed_by" => OptionalText,
                "claimed_until" => OptionalInteger, "created_at" => Integer,
                "updated_at" => Integer,
            ],
            ArchiveTable::WorkflowEffects => archive_columns![
                "id" => Uuid, "version" => Integer, "continuation_id" => Uuid,
                "sequence" => Integer, "attempt" => Integer,
                "request_json" => Text, "status" => Text, "result_json" => OptionalText,
                "message" => OptionalText, "idempotency_key" => Text, "created_at" => Integer,
                "updated_at" => Integer, "finished_at" => OptionalInteger,
                "current_executor_replica_id" => OptionalUuid,
                "last_executor_replica_id" => OptionalUuid,
            ],
            ArchiveTable::WorkflowEffectOutputEvents => archive_columns![
                "event_id" => Uuid, "effect_id" => Uuid, "attempt" => Integer,
                "output_json" => Text,
                "created_at" => Integer,
            ],
            ArchiveTable::WorkflowAiUsage => archive_columns![
                "event_id" => Uuid, "effect_id" => Uuid, "workflow_run_id" => Uuid,
                "workflow_id" => Uuid, "node_id" => OptionalText, "attempt" => Integer,
                "provider" => Text, "model" => Text, "input_tokens" => Integer,
                "cached_input_tokens" => Integer, "cache_creation_input_tokens" => Integer,
                "output_tokens" => Integer, "reasoning_tokens" => Integer,
                "cost_microusd" => OptionalInteger, "cost_source" => OptionalText,
                "recorded_at" => Integer,
            ],
            ArchiveTable::WorkflowEffectDispatches => archive_columns![
                "id" => Uuid, "effect_id" => Uuid, "dedupe_key" => Text,
                "command_json" => Text, "attempts" => Integer, "published_at" => OptionalInteger,
                "created_at" => Integer, "updated_at" => Integer, "last_error" => OptionalText,
                "claimed_by" => OptionalText, "claimed_until" => OptionalInteger,
                "available_at" => Integer,
            ],
            ArchiveTable::WorkflowJournalEntries => archive_columns![
                "id" => Uuid, "version" => Integer, "workflow_run_id" => Uuid,
                "sequence" => Integer, "continuation_id" => OptionalUuid,
                "effect_id" => OptionalUuid, "entry_json" => Text, "created_at" => Integer,
            ],
            ArchiveTable::WorkflowTriggerFirings => archive_columns![
                "id" => Uuid, "trigger_id" => Uuid, "fire_key" => Text,
                "workflow_run_id" => OptionalUuid, "scheduler_id" => Text,
                "created_at" => Integer, "outcome" => Text,
            ],
            ArchiveTable::PipelineRuns => archive_columns![
                "id" => Uuid, "pipeline_id" => Uuid, "pipeline_snapshot" => OptionalText,
                "status" => Text, "parameters" => Text, "state" => Text,
                "created_at" => Integer, "started_at" => OptionalInteger,
                "finished_at" => OptionalInteger, "message" => OptionalText,
                "trigger_source_kind" => OptionalText, "trigger_actor_type" => OptionalText,
                "trigger_actor_replica_id" => OptionalUuid,
                "trigger_actor_display_name" => OptionalText, "trigger_metadata" => Text,
                "orchestration_binding_id" => OptionalUuid, "execution_epoch" => OptionalInteger,
                "start_member" => OptionalText,
            ],
            ArchiveTable::PipelineMemberAttempts => archive_columns![
                "id" => Uuid, "pipeline_run_id" => Uuid, "member_key" => Text,
                "workflow_id" => Uuid, "attempt" => Integer, "workflow_run_id" => OptionalUuid,
                "status" => Text, "parameters" => Text, "result" => Text,
                "message" => OptionalText, "created_at" => Integer,
                "started_at" => OptionalInteger, "finished_at" => OptionalInteger,
            ],
            ArchiveTable::PipelineTriggerFirings => archive_columns![
                "id" => Uuid, "trigger_id" => Uuid, "fire_key" => Text,
                "pipeline_run_id" => OptionalUuid, "scheduler_id" => Text,
                "created_at" => Integer, "outcome" => Text,
            ],
            ArchiveTable::PipelineRevisions => archive_columns![
                "id" => Uuid, "pipeline_id" => Uuid, "revision" => Integer, "digest" => Text,
                "name" => Text, "description" => OptionalText, "graph" => Text,
                "concurrency" => Text, "defaults" => Text, "metadata" => Text,
                "source" => Text, "actor_id" => OptionalUuid, "actor_kind" => Text,
                "note" => OptionalText, "created_at" => Integer,
            ],
            ArchiveTable::Notifications => archive_columns![
                "id" => Uuid, "org_id" => OptionalUuid,
                "source_resource_type" => OptionalText,
                "source_resource_id" => OptionalUuid, "workflow_run_id" => OptionalUuid,
                "workflow_node_id" => OptionalText, "channel" => Text, "severity" => Text,
                "title" => Text, "body" => OptionalText, "target" => OptionalText,
                "metadata" => Text, "read_at" => OptionalInteger, "created_at" => Integer,
                "dedupe_key" => OptionalText,
            ],
            ArchiveTable::NotificationDeliveries => archive_columns![
                "id" => Uuid, "notification_id" => Uuid, "policy_id" => OptionalUuid,
                "channel" => Text, "target" => OptionalText, "provider" => OptionalText,
                "provider_function" => OptionalText,
                "workflow_run_id" => OptionalUuid, "status" => Text,
                "attempts" => Integer, "last_error" => OptionalText,
                "created_at" => Integer, "updated_at" => Integer,
                "dedupe_key" => OptionalText, "command_json" => OptionalText,
                "published_at" => OptionalInteger, "claimed_by" => OptionalText,
                "claimed_until" => OptionalInteger, "response_json" => OptionalText,
            ],
            ArchiveTable::AutomationRecords => archive_columns![
                "id" => Uuid, "record_type" => Text, "workflow_run_id" => OptionalUuid,
                "external_item_id" => OptionalUuid, "node_id" => OptionalText,
                "provider" => Text, "resource_type" => Text, "external_id" => Text,
                "status" => Text, "title" => OptionalText, "url" => OptionalText,
                "body" => OptionalText, "path" => OptionalText, "prompt" => OptionalText,
                "approval_type" => OptionalText, "resolved_by" => OptionalText,
                "resolved_at" => OptionalInteger, "metadata" => Text, "data" => Text,
                "created_at" => Integer, "updated_at" => Integer,
            ],
            ArchiveTable::Gates => archive_columns![
                "id" => Uuid, "workflow_run_id" => Uuid, "node_id" => Text, "kind" => Text,
                "status" => Text, "label" => OptionalText, "reason" => OptionalText,
                "resolved_by" => OptionalText, "resolved_at" => OptionalInteger,
                "metadata" => Text, "data" => Text, "created_at" => Integer,
                "updated_at" => Integer,
            ],
            ArchiveTable::OrgUsageLedger => archive_columns![
                "id" => Uuid, "org_id" => Uuid, "backend" => Text, "kind" => Text,
                "node_count" => Integer, "sampled_at" => Integer,
            ],
            ArchiveTable::WorkflowRevisions => archive_columns![
                "id" => Uuid, "workflow_id" => Uuid, "revision" => Integer, "version" => Text,
                "name" => Text, "definition" => Text, "input_schema" => Text, "output_schema" => OptionalText,
                "source" => Text, "actor_id" => OptionalUuid, "actor_kind" => Text,
                "note" => OptionalText, "created_at" => Integer, "digest" => Text,
            ],
            ArchiveTable::WorkflowFiles => archive_columns![
                "id" => Uuid, "scope" => Text, "org_id" => OptionalUuid,
                "owner_id" => OptionalUuid, "workflow_run_id" => OptionalUuid, "path" => Text,
                "name" => Text, "mime_type" => Text, "size_bytes" => Integer, "sha256" => Text,
                "uri" => Text, "revision" => Integer, "is_current" => Boolean,
                "archived" => Boolean, "created_at" => Integer,
            ],
            ArchiveTable::IngressAdmissions => archive_columns![
                "id" => Uuid, "org_scope" => Text, "scope" => Text,
                "correlation_key" => Text, "generation" => Integer,
                "workflow_id" => OptionalUuid, "pipeline_id" => OptionalUuid,
                "status" => Text, "workflow_run_id" => OptionalUuid,
                "pipeline_run_id" => OptionalUuid, "policy" => Text, "created_at" => Integer,
                "updated_at" => Integer,
            ],
            ArchiveTable::IngressEvents => archive_columns![
                "id" => Uuid, "admission_id" => Uuid, "sequence" => Integer,
                "generation" => Integer, "source" => Text, "event_id" => Text,
                "event_type" => Text, "correlation_key" => Text, "payload" => Text,
                "occurred_at" => OptionalInteger, "received_at" => Integer,
                "disposition" => Text, "queue_state" => Text, "claim_token" => OptionalUuid,
                "promoted_generation" => OptionalInteger, "workflow_run_id" => OptionalUuid,
                "pipeline_run_id" => OptionalUuid, "provenance" => Text,
            ],
            ArchiveTable::OrchestrationBindings => archive_columns![
                "id" => Uuid, "admission_id" => Uuid, "generation" => Integer,
                "pipeline_revision" => Integer, "pipeline_digest" => Text, "policy" => Text,
                "status" => Text, "current_phase" => OptionalText, "current_attempt" => Integer,
                "current_epoch" => Integer, "restart_member" => OptionalText,
                "resume_existing_epoch" => Integer, "subject_revision" => OptionalText,
                "resources" => Text, "budgets" => Text, "last_reduced_sequence" => Integer,
                "version" => Integer, "reducer_lease_owner" => OptionalText,
                "reducer_leased_until" => OptionalInteger, "created_at" => Integer,
                "updated_at" => Integer, "finished_at" => OptionalInteger,
                "adapter_id" => OptionalUuid, "adapter_revision" => OptionalInteger,
            ],
            ArchiveTable::OrchestrationEpochs => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "epoch" => Integer,
                "pipeline_run_id" => OptionalUuid, "start_member" => OptionalText,
                "parameters" => Text, "status" => Text, "reason" => Text,
                "created_at" => Integer, "started_at" => OptionalInteger,
                "finished_at" => OptionalInteger,
            ],
            ArchiveTable::OrchestrationEventReductions => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "inbox_event_id" => Uuid,
                "sequence" => Integer, "matched_intents" => Text, "winner" => OptionalText,
                "suppressed_intents" => Text, "binding_version" => Integer,
                "disposition" => Text, "detail" => Text, "created_at" => Integer,
            ],
            ArchiveTable::OrchestrationPendingIntents => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "intent" => Text, "priority" => Integer,
                "source_event_ids" => Text, "latest_payload" => Text, "wake_at" => Integer,
                "created_at" => Integer, "updated_at" => Integer,
            ],
            ArchiveTable::OrchestrationCommands => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "epoch" => Integer,
                "command_type" => Text, "operation_key" => Text, "payload" => Text,
                "status" => Text, "attempts" => Integer, "claimed_by" => OptionalText,
                "claimed_until" => OptionalInteger, "result" => Text, "created_at" => Integer,
                "updated_at" => Integer,
            ],
            ArchiveTable::OrchestrationEvidence => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "epoch" => OptionalInteger, "kind" => Text,
                "subject_revision" => OptionalText, "payload" => Text,
                "source_event_id" => OptionalUuid, "created_at" => Integer,
            ],
            ArchiveTable::ExternalOperations => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "operation_key" => Text,
                "provider" => Text, "action" => Text, "semantics" => Text,
                "attempt" => Integer, "status" => Text, "ambiguous" => Boolean,
                "provenance" => Text, "receipt" => Text, "created_at" => Integer,
                "updated_at" => Integer, "epoch" => Integer,
                "workflow_run_id" => OptionalUuid, "effect_id" => OptionalUuid,
            ],
            ArchiveTable::WorkspaceLeases => archive_columns![
                "id" => Uuid, "admission_id" => Uuid, "generation" => Integer, "scope" => Text,
                "attempt" => Integer, "worker_instance_id" => Text,
                "worker_replica_id" => OptionalUuid, "local_key" => Text,
                "requirements" => Text, "status" => Text, "version" => Integer,
                "leased_until" => Integer, "unavailable_since" => OptionalInteger,
                "evidence" => Text, "created_at" => Integer, "updated_at" => Integer,
                "abandonment_notified_at" => OptionalInteger,
            ],
            ArchiveTable::OrchestrationCorrelationAliases => archive_columns![
                "id" => Uuid, "binding_id" => Uuid, "org_scope" => Text,
                "source" => Text, "scope" => Text,
                "correlation_key" => Text, "created_at" => Integer, "updated_at" => Integer,
            ],
            ArchiveTable::AgentDirectives => archive_columns![
                "directive_id" => Uuid, "replica_id" => Uuid, "kind_json" => Text,
                "state" => Text, "issued_at" => Integer, "expires_at" => Integer,
                "published_at" => OptionalInteger, "completed_at" => OptionalInteger,
                "payload_json" => Text, "message" => OptionalText, "attempts" => Integer,
                "claimed_at" => OptionalInteger, "claimed_by_runtime_id" => OptionalText,
            ],
            ArchiveTable::DeadLetters => archive_columns![
                "id" => Uuid, "channel" => Text, "event_id" => OptionalUuid,
                "dedupe_key" => OptionalText, "attempts" => Integer, "error" => Text,
                "payload" => Text, "created_at" => Integer,
            ],
            ArchiveTable::AuditLog => archive_columns![
                "id" => Uuid, "actor_id" => OptionalUuid, "actor_kind" => Text,
                "action" => Text, "resource_type" => OptionalText,
                "resource_id" => OptionalUuid, "outcome" => Text, "detail" => OptionalText,
                "metadata" => Text, "created_at" => Integer,
            ],
            ArchiveTable::IdempotencyKeys => archive_columns![
                "id" => Uuid, "scope" => Text, "key" => Text, "result" => Text,
                "created_at" => Integer, "owner_node_run_id" => OptionalUuid,
                "claimed_at" => OptionalInteger, "completed_at" => OptionalInteger,
            ],
        }
    }

    fn archive_source_predicate(self) -> &'static str {
        match self {
            ArchiveTable::WorkflowRuns => {
                "status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND NOT EXISTS (SELECT 1 FROM workflow_vm_modules WHERE workflow_vm_modules.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM workflow_continuations WHERE workflow_continuations.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM workflow_journal_entries WHERE workflow_journal_entries.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM workflow_ai_usage WHERE workflow_ai_usage.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM workflow_trigger_firings WHERE workflow_trigger_firings.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM automation_records WHERE automation_records.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM gates WHERE gates.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM workflow_files WHERE workflow_files.workflow_run_id = workflow_runs.id) AND NOT EXISTS (SELECT 1 FROM pipeline_member_attempts WHERE pipeline_member_attempts.workflow_run_id = workflow_runs.id)"
            }
            ArchiveTable::WorkflowVmModules => {
                "EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_vm_modules.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')) AND NOT EXISTS (SELECT 1 FROM workflow_continuations WHERE workflow_continuations.workflow_run_id = workflow_vm_modules.workflow_run_id) AND NOT EXISTS (SELECT 1 FROM workflow_journal_entries WHERE workflow_journal_entries.workflow_run_id = workflow_vm_modules.workflow_run_id)"
            }
            ArchiveTable::WorkflowContinuations => {
                "EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_continuations.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled')) AND NOT EXISTS (SELECT 1 FROM workflow_effects WHERE workflow_effects.continuation_id = workflow_continuations.id)"
            }
            ArchiveTable::WorkflowEffects => {
                "status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND NOT EXISTS (SELECT 1 FROM workflow_effect_output_events WHERE workflow_effect_output_events.effect_id = workflow_effects.id) AND NOT EXISTS (SELECT 1 FROM workflow_ai_usage WHERE workflow_ai_usage.effect_id = workflow_effects.id) AND NOT EXISTS (SELECT 1 FROM workflow_effect_dispatches WHERE workflow_effect_dispatches.effect_id = workflow_effects.id)"
            }
            ArchiveTable::WorkflowEffectOutputEvents => {
                "EXISTS (SELECT 1 FROM workflow_effects e JOIN workflow_continuations c ON c.id = e.continuation_id JOIN workflow_runs r ON r.id = c.workflow_run_id WHERE e.id = workflow_effect_output_events.effect_id AND r.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::WorkflowAiUsage => {
                "EXISTS (SELECT 1 FROM workflow_runs r WHERE r.id = workflow_ai_usage.workflow_run_id AND r.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::WorkflowEffectDispatches => {
                "published_at IS NOT NULL OR (attempts > 0 AND last_error IS NOT NULL)"
            }
            ArchiveTable::WorkflowJournalEntries => {
                "EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_journal_entries.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::WorkflowTriggerFirings => {
                "workflow_run_id IS NULL OR EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_trigger_firings.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::PipelineRuns => {
                "status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND NOT EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.pipeline_run_id = pipeline_runs.id) AND NOT EXISTS (SELECT 1 FROM pipeline_trigger_firings WHERE pipeline_trigger_firings.pipeline_run_id = pipeline_runs.id) AND NOT EXISTS (SELECT 1 FROM pipeline_member_attempts WHERE pipeline_member_attempts.pipeline_run_id = pipeline_runs.id) AND NOT EXISTS (SELECT 1 FROM orchestration_epochs WHERE orchestration_epochs.pipeline_run_id = pipeline_runs.id)"
            }
            ArchiveTable::PipelineMemberAttempts => {
                "EXISTS (SELECT 1 FROM pipeline_runs WHERE pipeline_runs.id = pipeline_member_attempts.pipeline_run_id AND pipeline_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::PipelineTriggerFirings => {
                "pipeline_run_id IS NULL OR EXISTS (SELECT 1 FROM pipeline_runs WHERE pipeline_runs.id = pipeline_trigger_firings.pipeline_run_id AND pipeline_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::PipelineRevisions => {
                "revision < (SELECT MAX(newer.revision) FROM pipeline_revisions newer WHERE newer.pipeline_id = pipeline_revisions.pipeline_id)"
            }
            ArchiveTable::Notifications => {
                "NOT EXISTS (SELECT 1 FROM notification_deliveries WHERE notification_deliveries.notification_id = notifications.id)"
            }
            ArchiveTable::NotificationDeliveries => "status NOT IN ('pending', 'retrying')",
            ArchiveTable::AutomationRecords => {
                "resolved_at IS NOT NULL OR EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = automation_records.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::Gates => {
                "resolved_at IS NOT NULL OR EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = gates.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))"
            }
            ArchiveTable::OrgUsageLedger | ArchiveTable::DeadLetters | ArchiveTable::AuditLog => "",
            ArchiveTable::WorkflowRevisions => {
                "revision < (SELECT MAX(newer.revision) FROM workflow_revisions newer WHERE newer.workflow_id = workflow_revisions.workflow_id)"
            }
            ArchiveTable::WorkflowFiles => {
                "(scope = 'run' AND EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_runs.id = workflow_files.workflow_run_id AND workflow_runs.status IN ('succeeded', 'failed', 'timed_out', 'canceled'))) OR (scope = 'library' AND (archived = TRUE OR is_current = FALSE)) OR scope = 'staged'"
            }
            ArchiveTable::IngressAdmissions => {
                "status = 'terminal' AND NOT EXISTS (SELECT 1 FROM ingress_events WHERE ingress_events.admission_id = ingress_admissions.id) AND NOT EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.admission_id = ingress_admissions.id) AND NOT EXISTS (SELECT 1 FROM workspace_leases WHERE workspace_leases.admission_id = ingress_admissions.id)"
            }
            ArchiveTable::IngressEvents => {
                "EXISTS (SELECT 1 FROM ingress_admissions WHERE ingress_admissions.id = ingress_events.admission_id AND ingress_admissions.status = 'terminal') AND NOT EXISTS (SELECT 1 FROM orchestration_event_reductions WHERE orchestration_event_reductions.inbox_event_id = ingress_events.id) AND NOT EXISTS (SELECT 1 FROM orchestration_evidence WHERE orchestration_evidence.source_event_id = ingress_events.id)"
            }
            ArchiveTable::OrchestrationBindings => {
                "status IN ('completed', 'failed', 'terminated') AND NOT EXISTS (SELECT 1 FROM orchestration_epochs WHERE orchestration_epochs.binding_id = orchestration_bindings.id) AND NOT EXISTS (SELECT 1 FROM orchestration_event_reductions WHERE orchestration_event_reductions.binding_id = orchestration_bindings.id) AND NOT EXISTS (SELECT 1 FROM orchestration_pending_intents WHERE orchestration_pending_intents.binding_id = orchestration_bindings.id) AND NOT EXISTS (SELECT 1 FROM orchestration_commands WHERE orchestration_commands.binding_id = orchestration_bindings.id) AND NOT EXISTS (SELECT 1 FROM orchestration_evidence WHERE orchestration_evidence.binding_id = orchestration_bindings.id) AND NOT EXISTS (SELECT 1 FROM external_operations WHERE external_operations.binding_id = orchestration_bindings.id) AND NOT EXISTS (SELECT 1 FROM orchestration_correlation_aliases WHERE orchestration_correlation_aliases.binding_id = orchestration_bindings.id)"
            }
            ArchiveTable::OrchestrationEpochs => {
                "EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_epochs.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::OrchestrationEventReductions => {
                "EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_event_reductions.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::OrchestrationPendingIntents => {
                "EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_pending_intents.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::OrchestrationCommands => {
                "status IN ('succeeded', 'failed', 'superseded') AND EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_commands.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::OrchestrationEvidence => {
                "EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_evidence.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::ExternalOperations => {
                "status IN ('succeeded', 'failed') AND EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = external_operations.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::WorkspaceLeases => {
                "status IN ('released', 'abandoned') AND EXISTS (SELECT 1 FROM ingress_admissions WHERE ingress_admissions.id = workspace_leases.admission_id AND ingress_admissions.status = 'terminal')"
            }
            ArchiveTable::OrchestrationCorrelationAliases => {
                "EXISTS (SELECT 1 FROM orchestration_bindings WHERE orchestration_bindings.id = orchestration_correlation_aliases.binding_id AND orchestration_bindings.status IN ('completed', 'failed', 'terminated'))"
            }
            ArchiveTable::AgentDirectives => {
                "completed_at IS NOT NULL AND state IN ('completed', 'failed', 'unsupported', 'expired')"
            }
            ArchiveTable::IdempotencyKeys => {
                "completed_at IS NOT NULL OR owner_node_run_id IS NULL"
            }
        }
    }

    fn archive_row_json_v2<R>(self, row: &R) -> Result<Value, SendableError>
    where
        R: Row,
        for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> bool: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<i64>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<String>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<Uuid>: Decode<'r, R::Database> + Type<R::Database>,
        for<'c> &'c str: ColumnIndex<R>,
    {
        let mut object = Map::new();
        for column in self.archive_columns() {
            let value = match column.kind {
                ArchiveColumnKind::Uuid => {
                    Value::String(row.get::<Uuid, _>(column.name).to_string())
                }
                ArchiveColumnKind::OptionalUuid => row
                    .get::<Option<Uuid>, _>(column.name)
                    .map_or(Value::Null, |value| Value::String(value.to_string())),
                ArchiveColumnKind::Text => Value::String(row.get::<String, _>(column.name)),
                ArchiveColumnKind::OptionalText => row
                    .get::<Option<String>, _>(column.name)
                    .map_or(Value::Null, Value::String),
                ArchiveColumnKind::Integer => Value::from(row.get::<i64, _>(column.name)),
                ArchiveColumnKind::OptionalInteger => row
                    .get::<Option<i64>, _>(column.name)
                    .map_or(Value::Null, Value::from),
                ArchiveColumnKind::Boolean => Value::Bool(row.get::<bool, _>(column.name)),
            };
            object.insert(column.name.to_string(), value);
        }
        Ok(Value::Object(object))
    }

    fn archive_source_sql(self, dialect: SqlDialect) -> String {
        match self {
        ArchiveTable::WorkflowRuns => {
            "SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, watch_fired, run_metadata_json, extra_json, created_at, started_at, finished_at, message, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata FROM workflow_runs WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowVmModules => {
            "SELECT workflow_run_id, version, module_json, created_at FROM workflow_vm_modules WHERE workflow_run_id = ?".to_string()
        }
        ArchiveTable::WorkflowContinuations => {
            "SELECT id, workflow_run_id, module_version, continuation_json, status, version, ready_at, claimed_by, claimed_until, created_at, updated_at FROM workflow_continuations WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowEffects => {
            "SELECT id, version, continuation_id, sequence, attempt, request_json, status, result_json, message, idempotency_key, created_at, updated_at, finished_at, current_executor_replica_id, last_executor_replica_id FROM workflow_effects WHERE id = ? AND status IN ('succeeded', 'failed', 'timed_out', 'canceled')".to_string()
        }
        ArchiveTable::WorkflowEffectOutputEvents => {
            "SELECT event_id, effect_id, attempt, output_json, created_at FROM workflow_effect_output_events WHERE event_id = ?".to_string()
        }
        ArchiveTable::WorkflowAiUsage => {
            "SELECT event_id, effect_id, workflow_run_id, workflow_id, node_id, attempt, provider, model, input_tokens, cached_input_tokens, cache_creation_input_tokens, output_tokens, reasoning_tokens, cost_microusd, cost_source, recorded_at FROM workflow_ai_usage WHERE event_id = ?".to_string()
        }
        ArchiveTable::WorkflowEffectDispatches => {
            "SELECT id, effect_id, dedupe_key, command_json, attempts, published_at, created_at, updated_at, last_error, claimed_by, claimed_until FROM workflow_effect_dispatches WHERE id = ? AND (published_at IS NOT NULL OR (attempts > 0 AND last_error IS NOT NULL))".to_string()
        }
        ArchiveTable::WorkflowJournalEntries => {
            "SELECT id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at FROM workflow_journal_entries WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowTriggerFirings => {
            "SELECT id, trigger_id, fire_key, workflow_run_id, scheduler_id, created_at, outcome FROM workflow_trigger_firings WHERE id = ?".to_string()
        }
        ArchiveTable::PipelineRuns => {
            "SELECT id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, started_at, finished_at, message, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata FROM pipeline_runs WHERE id = ?".to_string()
        }
        ArchiveTable::PipelineTriggerFirings => {
            "SELECT id, trigger_id, fire_key, pipeline_run_id, scheduler_id, created_at, outcome FROM pipeline_trigger_firings WHERE id = ?".to_string()
        }
        ArchiveTable::Notifications => {
            "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications WHERE id = ?".to_string()
        }
        ArchiveTable::NotificationDeliveries => {
            "SELECT id, notification_id, policy_id, channel, target, provider, provider_function, workflow_run_id, status, attempts, last_error, created_at, updated_at, dedupe_key, command_json, published_at, claimed_by, claimed_until, response_json FROM notification_deliveries WHERE id = ? AND status NOT IN ('pending', 'retrying')".to_string()
        }
        ArchiveTable::AutomationRecords => {
            "SELECT id, record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at FROM automation_records WHERE id = ?".to_string()
        }
        ArchiveTable::Gates => {
            "SELECT id, workflow_run_id, node_id, kind, status, label, reason, resolved_by, resolved_at, metadata, data, created_at, updated_at FROM gates WHERE id = ?".to_string()
        }
        ArchiveTable::OrgUsageLedger => {
            "SELECT id, org_id, backend, kind, node_count, sampled_at FROM org_usage_ledger WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowRevisions => {
            "SELECT id, workflow_id, revision, version, name, definition, input_schema, output_schema, source, actor_id, actor_kind, note, created_at FROM workflow_revisions WHERE id = ?".to_string()
        }
        ArchiveTable::AgentDirectives => {
            "SELECT directive_id, replica_id, kind_json, state, issued_at, expires_at, published_at, completed_at, payload_json, message, attempts, claimed_at, claimed_by_runtime_id FROM agent_directives WHERE directive_id = ? AND completed_at IS NOT NULL".to_string()
        }
        ArchiveTable::DeadLetters => {
            "SELECT id, channel, event_id, dedupe_key, attempts, error, payload, created_at FROM dead_letters WHERE id = ?".to_string()
        }
        ArchiveTable::AuditLog => {
            "SELECT id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at FROM audit_log WHERE id = ?".to_string()
        }
        ArchiveTable::IdempotencyKeys => {
            format!(
                "SELECT id, scope, {key_col}, result, created_at FROM idempotency_keys WHERE id = ?",
                key_col = dialect.ident("key")
            )
        }
        _ => self.archive_source_sql_v2(dialect),
    }
    }

    fn archive_row_json<R>(self, row: &R) -> Result<Value, SendableError>
    where
        R: Row,
        for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> bool: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<i64>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<String>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<Uuid>: Decode<'r, R::Database> + Type<R::Database>,
        for<'c> &'c str: ColumnIndex<R>,
    {
        Ok(match self {
            ArchiveTable::WorkflowRuns => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "workflow_id": row.get::<Uuid, _>("workflow_id").to_string(),
                "workflow_snapshot": row.get::<Option<String>, _>("workflow_snapshot"),
                "status": row.get::<String, _>("status"),
                "active_node_id": row.get::<Option<String>, _>("active_node_id"),
                "parameters": row.get::<String, _>("parameters"),
                "watch_fired": row.get::<bool, _>("watch_fired"),
                "run_metadata_json": row.get::<Option<String>, _>("run_metadata_json"),
                "extra_json": row.get::<String, _>("extra_json"),
                "created_at": row.get::<i64, _>("created_at"),
                "started_at": row.get::<Option<i64>, _>("started_at"),
                "finished_at": row.get::<Option<i64>, _>("finished_at"),
                "message": row.get::<Option<String>, _>("message"),
                "name": row.get::<Option<String>, _>("name"),
                "trigger_source_kind": row.get::<Option<String>, _>("trigger_source_kind"),
                "trigger_actor_type": row.get::<Option<String>, _>("trigger_actor_type"),
                "trigger_actor_replica_id": row.get::<Option<Uuid>, _>("trigger_actor_replica_id").map(|id| id.to_string()),
                "trigger_actor_display_name": row.get::<Option<String>, _>("trigger_actor_display_name"),
                "trigger_request_host": row.get::<Option<String>, _>("trigger_request_host"),
                "trigger_request_ip": row.get::<Option<String>, _>("trigger_request_ip"),
                "trigger_metadata": row.get::<String, _>("trigger_metadata"),
            }),
            ArchiveTable::WorkflowVmModules => runinator_models::json!({
                "workflow_run_id": row.get::<Uuid, _>("workflow_run_id").to_string(),
                "version": row.get::<i64, _>("version"),
                "module_json": row.get::<String, _>("module_json"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::WorkflowContinuations => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "workflow_run_id": row.get::<Uuid, _>("workflow_run_id").to_string(),
                "module_version": row.get::<i64, _>("module_version"),
                "continuation_json": row.get::<String, _>("continuation_json"),
                "status": row.get::<String, _>("status"),
                "version": row.get::<i64, _>("version"),
                "ready_at": row.get::<Option<i64>, _>("ready_at"),
                "claimed_by": row.get::<Option<String>, _>("claimed_by"),
                "claimed_until": row.get::<Option<i64>, _>("claimed_until"),
                "created_at": row.get::<i64, _>("created_at"),
                "updated_at": row.get::<i64, _>("updated_at"),
            }),
            ArchiveTable::WorkflowEffects => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "version": row.get::<i64, _>("version"),
                "continuation_id": row.get::<Uuid, _>("continuation_id").to_string(),
                "sequence": row.get::<i64, _>("sequence"),
                "attempt": row.get::<i64, _>("attempt"),
                "request_json": row.get::<String, _>("request_json"),
                "status": row.get::<String, _>("status"),
                "result_json": row.get::<Option<String>, _>("result_json"),
                "message": row.get::<Option<String>, _>("message"),
                "idempotency_key": row.get::<String, _>("idempotency_key"),
                "created_at": row.get::<i64, _>("created_at"),
                "updated_at": row.get::<i64, _>("updated_at"),
                "finished_at": row.get::<Option<i64>, _>("finished_at"),
                "current_executor_replica_id": row.get::<Option<Uuid>, _>("current_executor_replica_id").map(|id| id.to_string()),
                "last_executor_replica_id": row.get::<Option<Uuid>, _>("last_executor_replica_id").map(|id| id.to_string()),
            }),
            ArchiveTable::WorkflowEffectOutputEvents => runinator_models::json!({
                "event_id": row.get::<Uuid, _>("event_id").to_string(),
                "effect_id": row.get::<Uuid, _>("effect_id").to_string(),
                "attempt": row.get::<i64, _>("attempt"),
                "output_json": row.get::<String, _>("output_json"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::WorkflowAiUsage => runinator_models::json!({
                "event_id": row.get::<Uuid, _>("event_id").to_string(),
                "effect_id": row.get::<Uuid, _>("effect_id").to_string(),
                "workflow_run_id": row.get::<Uuid, _>("workflow_run_id").to_string(),
                "workflow_id": row.get::<Uuid, _>("workflow_id").to_string(),
                "node_id": row.get::<Option<String>, _>("node_id"),
                "attempt": row.get::<i64, _>("attempt"),
                "provider": row.get::<String, _>("provider"),
                "model": row.get::<String, _>("model"),
                "input_tokens": row.get::<i64, _>("input_tokens"),
                "cached_input_tokens": row.get::<i64, _>("cached_input_tokens"),
                "cache_creation_input_tokens": row.get::<i64, _>("cache_creation_input_tokens"),
                "output_tokens": row.get::<i64, _>("output_tokens"),
                "reasoning_tokens": row.get::<i64, _>("reasoning_tokens"),
                "cost_microusd": row.get::<Option<i64>, _>("cost_microusd"),
                "cost_source": row.get::<Option<String>, _>("cost_source"),
                "recorded_at": row.get::<i64, _>("recorded_at"),
            }),
            ArchiveTable::WorkflowEffectDispatches => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "effect_id": row.get::<Uuid, _>("effect_id").to_string(),
                "dedupe_key": row.get::<String, _>("dedupe_key"),
                "command_json": row.get::<String, _>("command_json"),
                "attempts": row.get::<i64, _>("attempts"),
                "published_at": row.get::<Option<i64>, _>("published_at"),
                "created_at": row.get::<i64, _>("created_at"),
                "updated_at": row.get::<i64, _>("updated_at"),
                "last_error": row.get::<Option<String>, _>("last_error"),
                "claimed_by": row.get::<Option<String>, _>("claimed_by"),
                "claimed_until": row.get::<Option<i64>, _>("claimed_until"),
            }),
            ArchiveTable::WorkflowJournalEntries => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "version": row.get::<i64, _>("version"),
                "workflow_run_id": row.get::<Uuid, _>("workflow_run_id").to_string(),
                "sequence": row.get::<i64, _>("sequence"),
                "continuation_id": row.get::<Option<Uuid>, _>("continuation_id").map(|id| id.to_string()),
                "effect_id": row.get::<Option<Uuid>, _>("effect_id").map(|id| id.to_string()),
                "entry_json": row.get::<String, _>("entry_json"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::WorkflowTriggerFirings => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "trigger_id": row.get::<Uuid, _>("trigger_id").to_string(),
                "fire_key": row.get::<String, _>("fire_key"),
                "workflow_run_id": row.get::<Option<Uuid>, _>("workflow_run_id").map(|id| id.to_string()),
                "scheduler_id": row.get::<String, _>("scheduler_id"), "created_at": row.get::<i64, _>("created_at"),
                "outcome": row.get::<String, _>("outcome"),
            }),
            ArchiveTable::PipelineRuns => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "pipeline_id": row.get::<Uuid, _>("pipeline_id").to_string(),
                "pipeline_snapshot": row.get::<Option<String>, _>("pipeline_snapshot"), "status": row.get::<String, _>("status"),
                "parameters": row.get::<String, _>("parameters"), "state": row.get::<String, _>("state"),
                "created_at": row.get::<i64, _>("created_at"), "started_at": row.get::<Option<i64>, _>("started_at"),
                "finished_at": row.get::<Option<i64>, _>("finished_at"), "message": row.get::<Option<String>, _>("message"),
                "trigger_source_kind": row.get::<Option<String>, _>("trigger_source_kind"),
                "trigger_actor_type": row.get::<Option<String>, _>("trigger_actor_type"),
                "trigger_actor_replica_id": row.get::<Option<Uuid>, _>("trigger_actor_replica_id").map(|id| id.to_string()),
                "trigger_actor_display_name": row.get::<Option<String>, _>("trigger_actor_display_name"),
                "trigger_metadata": row.get::<String, _>("trigger_metadata"),
            }),
            ArchiveTable::PipelineTriggerFirings => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "trigger_id": row.get::<Uuid, _>("trigger_id").to_string(),
                "fire_key": row.get::<String, _>("fire_key"),
                "pipeline_run_id": row.get::<Option<Uuid>, _>("pipeline_run_id").map(|id| id.to_string()),
                "scheduler_id": row.get::<String, _>("scheduler_id"), "created_at": row.get::<i64, _>("created_at"),
                "outcome": row.get::<String, _>("outcome"),
            }),
            ArchiveTable::Notifications => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "workflow_run_id": row.get::<Option<Uuid>, _>("workflow_run_id").map(|id| id.to_string()),
                "workflow_node_id": row.get::<Option<String>, _>("workflow_node_id"),
                "channel": row.get::<String, _>("channel"),
                "severity": row.get::<String, _>("severity"),
                "title": row.get::<String, _>("title"),
                "body": row.get::<Option<String>, _>("body"),
                "target": row.get::<Option<String>, _>("target"),
                "metadata": row.get::<String, _>("metadata"),
                "read_at": row.get::<Option<i64>, _>("read_at"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::NotificationDeliveries => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "notification_id": row.get::<Uuid, _>("notification_id").to_string(),
                "policy_id": row.get::<Option<Uuid>, _>("policy_id").map(|id| id.to_string()),
                "channel": row.get::<String, _>("channel"), "target": row.get::<Option<String>, _>("target"),
                "provider": row.get::<Option<String>, _>("provider"), "provider_function": row.get::<Option<String>, _>("provider_function"),
                "workflow_run_id": row.get::<Option<Uuid>, _>("workflow_run_id").map(|id| id.to_string()),
                "status": row.get::<String, _>("status"), "attempts": row.get::<i64, _>("attempts"),
                "last_error": row.get::<Option<String>, _>("last_error"), "created_at": row.get::<i64, _>("created_at"),
                "updated_at": row.get::<i64, _>("updated_at"),
                "dedupe_key": row.get::<Option<String>, _>("dedupe_key"),
                "command_json": row.get::<Option<String>, _>("command_json"),
                "published_at": row.get::<Option<i64>, _>("published_at"),
                "claimed_by": row.get::<Option<String>, _>("claimed_by"),
                "claimed_until": row.get::<Option<i64>, _>("claimed_until"),
                "response_json": row.get::<Option<String>, _>("response_json"),
            }),
            ArchiveTable::AutomationRecords => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "record_type": row.get::<String, _>("record_type"),
                "workflow_run_id": row.get::<Option<Uuid>, _>("workflow_run_id").map(|id| id.to_string()),
                "external_item_id": row.get::<Option<Uuid>, _>("external_item_id").map(|id| id.to_string()),
                "node_id": row.get::<Option<String>, _>("node_id"), "provider": row.get::<String, _>("provider"),
                "resource_type": row.get::<String, _>("resource_type"), "external_id": row.get::<String, _>("external_id"),
                "status": row.get::<String, _>("status"), "title": row.get::<Option<String>, _>("title"),
                "url": row.get::<Option<String>, _>("url"), "body": row.get::<Option<String>, _>("body"),
                "path": row.get::<Option<String>, _>("path"), "prompt": row.get::<Option<String>, _>("prompt"),
                "approval_type": row.get::<Option<String>, _>("approval_type"), "resolved_by": row.get::<Option<String>, _>("resolved_by"),
                "resolved_at": row.get::<Option<i64>, _>("resolved_at"), "metadata": row.get::<String, _>("metadata"),
                "data": row.get::<String, _>("data"), "created_at": row.get::<i64, _>("created_at"), "updated_at": row.get::<i64, _>("updated_at"),
            }),
            ArchiveTable::Gates => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "workflow_run_id": row.get::<Uuid, _>("workflow_run_id").to_string(),
                "node_id": row.get::<String, _>("node_id"), "kind": row.get::<String, _>("kind"), "status": row.get::<String, _>("status"),
                "label": row.get::<Option<String>, _>("label"), "reason": row.get::<Option<String>, _>("reason"),
                "resolved_by": row.get::<Option<String>, _>("resolved_by"), "resolved_at": row.get::<Option<i64>, _>("resolved_at"),
                "metadata": row.get::<String, _>("metadata"), "data": row.get::<String, _>("data"),
                "created_at": row.get::<i64, _>("created_at"), "updated_at": row.get::<i64, _>("updated_at"),
            }),
            ArchiveTable::OrgUsageLedger => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "org_id": row.get::<Uuid, _>("org_id").to_string(),
                "backend": row.get::<String, _>("backend"), "kind": row.get::<String, _>("kind"),
                "node_count": row.get::<i64, _>("node_count"), "sampled_at": row.get::<i64, _>("sampled_at"),
            }),
            ArchiveTable::WorkflowRevisions => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(), "workflow_id": row.get::<Uuid, _>("workflow_id").to_string(),
                "revision": row.get::<i64, _>("revision"), "version": row.get::<String, _>("version"),
                "name": row.get::<String, _>("name"), "definition": row.get::<String, _>("definition"),
                "input_schema": row.get::<String, _>("input_schema"), "output_schema": row.get::<Option<String>, _>("output_schema"), "source": row.get::<String, _>("source"),
                "actor_id": row.get::<Option<Uuid>, _>("actor_id").map(|id| id.to_string()), "actor_kind": row.get::<String, _>("actor_kind"),
                "note": row.get::<Option<String>, _>("note"), "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::AgentDirectives => runinator_models::json!({
                "directive_id": row.get::<Uuid, _>("directive_id").to_string(), "replica_id": row.get::<Uuid, _>("replica_id").to_string(),
                "kind_json": row.get::<String, _>("kind_json"), "state": row.get::<String, _>("state"),
                "issued_at": row.get::<i64, _>("issued_at"), "expires_at": row.get::<i64, _>("expires_at"),
                "published_at": row.get::<Option<i64>, _>("published_at"), "completed_at": row.get::<Option<i64>, _>("completed_at"),
                "payload_json": row.get::<String, _>("payload_json"), "message": row.get::<Option<String>, _>("message"),
                "attempts": row.get::<i64, _>("attempts"), "claimed_at": row.get::<Option<i64>, _>("claimed_at"),
                "claimed_by_runtime_id": row.get::<Option<String>, _>("claimed_by_runtime_id"),
            }),
            ArchiveTable::DeadLetters => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "channel": row.get::<String, _>("channel"),
                "event_id": row.get::<Option<Uuid>, _>("event_id").map(|id| id.to_string()),
                "dedupe_key": row.get::<Option<String>, _>("dedupe_key"),
                "attempts": row.get::<i64, _>("attempts"),
                "error": row.get::<String, _>("error"),
                "payload": row.get::<String, _>("payload"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::AuditLog => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "actor_id": row.get::<Option<Uuid>, _>("actor_id").map(|id| id.to_string()),
                "actor_kind": row.get::<String, _>("actor_kind"),
                "action": row.get::<String, _>("action"),
                "resource_type": row.get::<Option<String>, _>("resource_type"),
                "resource_id": row.get::<Option<Uuid>, _>("resource_id").map(|id| id.to_string()),
                "outcome": row.get::<String, _>("outcome"),
                "detail": row.get::<Option<String>, _>("detail"),
                "metadata": row.get::<String, _>("metadata"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::IdempotencyKeys => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "scope": row.get::<String, _>("scope"),
                "key": row.get::<String, _>("key"),
                "result": row.get::<String, _>("result"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            _ => return self.archive_row_json_v2(row),
        })
    }
}
