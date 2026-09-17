//! Generic placement and recovery policy for admission-scoped worker-local workspaces.

use std::{collections::BTreeMap, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use runinator_models::{
    errors::SendableError,
    replicas::{ReplicaKind, ReplicaRecord, ReplicaStatus},
    value::Value,
    workspaces::{NewWorkspaceLease, WorkspaceLease, WorkspaceStatus},
};
use runinator_store::roles::{ReplicaStore, WorkspaceStore};
use uuid::Uuid;

pub const DEFAULT_WORKSPACE_LEASE_SECONDS: i64 = 15 * 60;
pub const DEFAULT_WORKER_LOSS_GRACE_SECONDS: i64 = 10 * 60;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceRecovery {
    Rebound(WorkspaceLease),
    Waiting(WorkspaceLease),
    Abandoned(WorkspaceLease),
}

fn validate_allocation(request: &WorkspaceAllocationRequest) -> Result<(), SendableError> {
    if request.scope.trim().is_empty() || request.generation < 1 || request.attempt < 1 {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace generation, attempt, and scope are required",
        )));
    }
    Ok(())
}

fn safe_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn labels_match(replica: &ReplicaRecord, required: &BTreeMap<String, String>) -> bool {
    let labels = replica.attributes.get("labels").and_then(Value::as_object);
    required.iter().all(|(key, expected)| {
        labels
            .and_then(|labels| labels.get(key))
            .and_then(Value::as_str)
            .is_some_and(|actual| actual == expected)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_names_become_safe_local_key_segments() {
        assert_eq!(safe_segment("phase / shard"), "phase---shard");
        assert_eq!(safe_segment("processing"), "processing");
    }
}

mod workspace_allocation_request;
pub use workspace_allocation_request::WorkspaceAllocationRequest;

mod workspace_operations;
pub use workspace_operations::WorkspaceOperations;
