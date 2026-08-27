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

#[derive(Debug, Clone)]
pub struct WorkspaceAllocationRequest {
    pub admission_id: Uuid,
    pub generation: i64,
    pub scope: String,
    pub attempt: i64,
    pub required_labels: BTreeMap<String, String>,
    pub lease_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceRecovery {
    Rebound(WorkspaceLease),
    Waiting(WorkspaceLease),
    Abandoned(WorkspaceLease),
}

#[derive(Clone)]
pub struct WorkspaceOperations<T> {
    store: Arc<T>,
}

impl<T> WorkspaceOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: WorkspaceStore + ReplicaStore> WorkspaceOperations<T> {
    pub async fn allocate(
        &self,
        request: WorkspaceAllocationRequest,
    ) -> Result<WorkspaceLease, SendableError> {
        if let Some(existing) = self
            .store
            .fetch_workspace_attempt(
                request.admission_id,
                request.generation,
                request.scope.clone(),
                request.attempt,
            )
            .await?
        {
            return Ok(existing);
        }
        validate_allocation(&request)?;
        let now = Utc::now();
        let replicas = self
            .store
            .fetch_replicas(
                Some(ReplicaKind::Worker),
                Some(ReplicaStatus::Live),
                now - Duration::seconds(90),
            )
            .await?;
        let loads = self.store.count_running_effects_by_executor().await?;
        let mut candidates = Vec::new();
        for replica in replicas {
            if !labels_match(&replica, &request.required_labels) {
                continue;
            }
            let load = loads
                .iter()
                .find_map(|(id, count)| (*id == replica.replica_id).then_some(*count))
                .unwrap_or_default();
            candidates.push((load, replica.instance_id.clone(), replica));
        }
        candidates.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.replica_id.cmp(&right.2.replica_id))
        });
        let Some((_, _, selected)) = candidates.into_iter().next() else {
            return Err(Box::new(std::io::Error::other(
                "no live worker satisfies the requested workspace labels",
            )));
        };
        let id = Uuid::now_v7();
        let local_key = format!(
            "admissions/{}/{}/{}/{}-{id}",
            request.admission_id,
            request.generation,
            safe_segment(&request.scope),
            request.attempt,
        );
        let requirements = Value::from(serde_json::json!({
            "labels": request.required_labels,
        }));
        self.store
            .allocate_workspace(NewWorkspaceLease {
                id,
                admission_id: request.admission_id,
                generation: request.generation,
                scope: request.scope,
                attempt: request.attempt,
                worker_instance_id: selected.instance_id,
                worker_replica_id: Some(selected.replica_id),
                local_key,
                requirements,
                leased_until: now
                    + Duration::seconds(
                        request
                            .lease_seconds
                            .unwrap_or(DEFAULT_WORKSPACE_LEASE_SECONDS)
                            .max(60),
                    ),
            })
            .await
    }

    pub async fn activate(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        self.transition(
            workspace_id,
            expected_version,
            WorkspaceStatus::Allocating,
            WorkspaceStatus::Active,
            None,
        )
        .await
    }

    pub async fn begin_finalization(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        let Some(current) = self.store.fetch_workspace(workspace_id).await? else {
            return Ok(None);
        };
        if current.status.is_terminal() || current.version != expected_version {
            return Ok(None);
        }
        self.transition(
            workspace_id,
            expected_version,
            current.status,
            WorkspaceStatus::Finalizing,
            None,
        )
        .await
    }

    pub async fn release(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        evidence: Value,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        self.transition(
            workspace_id,
            expected_version,
            WorkspaceStatus::Finalizing,
            WorkspaceStatus::Released,
            Some(evidence),
        )
        .await
    }

    /// Fence an explicitly canceled or superseded workspace without assuming why it was stopped.
    pub async fn abandon(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        evidence: Value,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        let Some(current) = self.store.fetch_workspace(workspace_id).await? else {
            return Ok(None);
        };
        if current.status.is_terminal() || current.version != expected_version {
            return Ok(None);
        }
        self.transition(
            workspace_id,
            expected_version,
            current.status,
            WorkspaceStatus::Abandoned,
            Some(evidence),
        )
        .await
    }

    async fn transition(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        expected_status: WorkspaceStatus,
        next_status: WorkspaceStatus,
        evidence: Option<Value>,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        if !self
            .store
            .transition_workspace_cas(
                workspace_id,
                expected_version,
                expected_status,
                next_status,
                evidence,
                Utc::now(),
            )
            .await?
        {
            return Ok(None);
        }
        self.store.fetch_workspace(workspace_id).await
    }

    /// Rebind returned instances and abandon workspaces whose stable instance exceeded its grace.
    /// The admission coordinator reschedules the logical scope for each `Abandoned` result.
    pub async fn reconcile_expired(
        &self,
        now: DateTime<Utc>,
        loss_grace_seconds: Option<i64>,
        limit: i64,
    ) -> Result<Vec<WorkspaceRecovery>, SendableError> {
        let live = self
            .store
            .fetch_replicas(
                Some(ReplicaKind::Worker),
                Some(ReplicaStatus::Live),
                now - Duration::seconds(90),
            )
            .await?;
        let grace = Duration::seconds(
            loss_grace_seconds
                .unwrap_or(DEFAULT_WORKER_LOSS_GRACE_SECONDS)
                .max(0),
        );
        let mut outcomes = Vec::new();
        for workspace in self.store.fetch_expired_workspaces(now, limit).await? {
            if let Some(replica) = live
                .iter()
                .find(|replica| replica.instance_id == workspace.worker_instance_id)
            {
                if self
                    .store
                    .renew_workspace(
                        workspace.id,
                        workspace.version,
                        workspace.worker_instance_id.clone(),
                        Some(replica.replica_id),
                        now + Duration::seconds(DEFAULT_WORKSPACE_LEASE_SECONDS),
                        now,
                    )
                    .await?
                    && let Some(updated) = self.store.fetch_workspace(workspace.id).await?
                {
                    outcomes.push(WorkspaceRecovery::Rebound(updated));
                }
                continue;
            }
            if workspace.unavailable_since.is_none() {
                self.store
                    .mark_workspace_unavailable(workspace.worker_instance_id.clone(), now)
                    .await?;
                let updated = self
                    .store
                    .fetch_workspace(workspace.id)
                    .await?
                    .unwrap_or(workspace);
                outcomes.push(WorkspaceRecovery::Waiting(updated));
                continue;
            }
            if workspace
                .unavailable_since
                .is_some_and(|since| since + grace > now)
            {
                outcomes.push(WorkspaceRecovery::Waiting(workspace));
                continue;
            }
            if self
                .store
                .transition_workspace_cas(
                    workspace.id,
                    workspace.version,
                    workspace.status,
                    WorkspaceStatus::Abandoned,
                    Some(Value::from(serde_json::json!({
                        "evidence_lost": true,
                        "reason": "worker instance did not return within the recovery grace period",
                    }))),
                    now,
                )
                .await?
                && let Some(updated) = self.store.fetch_workspace(workspace.id).await?
            {
                outcomes.push(WorkspaceRecovery::Abandoned(updated));
            }
        }
        Ok(outcomes)
    }
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
