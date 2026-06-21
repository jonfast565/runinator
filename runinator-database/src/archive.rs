use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use runinator_models::value::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ArchiveTable {
    WorkflowRuns,
    WorkflowNodeChunks,
    WorkflowReadyNodes,
    RunChunks,
    WorkflowActionDispatches,
    Notifications,
    DeadLetters,
    AuditLog,
    IdempotencyKeys,
}

impl ArchiveTable {
    pub const ALL: [ArchiveTable; 9] = [
        ArchiveTable::WorkflowRuns,
        ArchiveTable::WorkflowNodeChunks,
        ArchiveTable::WorkflowReadyNodes,
        ArchiveTable::RunChunks,
        ArchiveTable::WorkflowActionDispatches,
        ArchiveTable::Notifications,
        ArchiveTable::DeadLetters,
        ArchiveTable::AuditLog,
        ArchiveTable::IdempotencyKeys,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveTable::WorkflowRuns => "workflow_runs",
            ArchiveTable::WorkflowNodeChunks => "workflow_node_chunks",
            ArchiveTable::WorkflowReadyNodes => "workflow_ready_nodes",
            ArchiveTable::RunChunks => "run_chunks",
            ArchiveTable::WorkflowActionDispatches => "workflow_action_dispatches",
            ArchiveTable::Notifications => "notifications",
            ArchiveTable::DeadLetters => "dead_letters",
            ArchiveTable::AuditLog => "audit_log",
            ArchiveTable::IdempotencyKeys => "idempotency_keys",
        }
    }

    pub fn primary_key_column(self) -> &'static str {
        "id"
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
            "workflow_runs" => Ok(ArchiveTable::WorkflowRuns),
            "workflow_node_chunks" => Ok(ArchiveTable::WorkflowNodeChunks),
            "workflow_ready_nodes" => Ok(ArchiveTable::WorkflowReadyNodes),
            "run_chunks" => Ok(ArchiveTable::RunChunks),
            "workflow_action_dispatches" => Ok(ArchiveTable::WorkflowActionDispatches),
            "notifications" => Ok(ArchiveTable::Notifications),
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
