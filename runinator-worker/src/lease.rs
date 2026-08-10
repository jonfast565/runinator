use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_comm::ActionCommand;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

const EXECUTOR_LEASE_GRACE: chrono::Duration = chrono::Duration::seconds(60);

#[derive(Default)]
pub(crate) struct StaleLeaseRegistry(Mutex<HashMap<Uuid, i64>>);

impl StaleLeaseRegistry {
    pub(crate) async fn record(&self, node_run_id: Uuid, min_reclaim_attempt: i64) {
        self.0.lock().await.insert(node_run_id, min_reclaim_attempt);
    }

    pub(crate) async fn clear(&self, node_run_id: Uuid) {
        self.0.lock().await.remove(&node_run_id);
    }

    pub(crate) async fn matches(&self, node_run_id: Uuid, attempt: i64) -> bool {
        self.0
            .lock()
            .await
            .get(&node_run_id)
            .is_some_and(|min_reclaim| *min_reclaim <= attempt)
    }
}

/// owns executor-lease identity, policy, and recovery state for one worker runtime.
#[derive(Clone)]
pub(crate) struct ExecutorLeaseManager {
    api_client: AsyncApiClient<StaticLocator>,
    replica_id: Option<Uuid>,
    stale: Arc<StaleLeaseRegistry>,
}

impl ExecutorLeaseManager {
    pub(crate) fn new(api_client: AsyncApiClient<StaticLocator>, replica_id: Option<Uuid>) -> Self {
        Self {
            api_client,
            replica_id,
            stale: Arc::new(StaleLeaseRegistry::default()),
        }
    }

    /// claim this delivery's lease, returning whether another executor still owns it.
    pub(crate) async fn held_elsewhere(&self, command: &ActionCommand) -> bool {
        let Some(replica_id) = self.replica_id else {
            return false;
        };
        let now = Utc::now();
        let stale_before =
            now - chrono::Duration::seconds(command.action.timeout_seconds) - EXECUTOR_LEASE_GRACE;
        let mut held_elsewhere = false;
        match self
            .api_client
            .claim_workflow_node_run_executor(
                command.workflow_node_run_id,
                replica_id,
                now,
                stale_before,
            )
            .await
        {
            Ok(true) => self.stale.clear(command.workflow_node_run_id).await,
            Ok(false) => held_elsewhere = true,
            Err(err) => warn!(
                replica_id = %replica_id,
                node_run_id = %command.workflow_node_run_id,
                "failed to claim executor: {}",
                err
            ),
        }

        if held_elsewhere
            && self
                .stale
                .matches(command.workflow_node_run_id, command.attempt)
                .await
            && self
                .api_client
                .release_workflow_node_run_executor(
                    command.workflow_node_run_id,
                    replica_id,
                    Utc::now(),
                )
                .await
                .is_ok()
        {
            match self
                .api_client
                .claim_workflow_node_run_executor(
                    command.workflow_node_run_id,
                    replica_id,
                    Utc::now(),
                    stale_before,
                )
                .await
            {
                Ok(acquired) => {
                    held_elsewhere = !acquired;
                    if acquired {
                        self.stale.clear(command.workflow_node_run_id).await;
                        info!(
                            node_run_id = %command.workflow_node_run_id,
                            "reclaimed own executor lease left behind by a failed release"
                        );
                    }
                }
                Err(err) => {
                    held_elsewhere = false;
                    warn!(
                        replica_id = %replica_id,
                        node_run_id = %command.workflow_node_run_id,
                        "failed to reclaim executor after releasing own stale lease: {}",
                        err
                    );
                }
            }
        }
        held_elsewhere
    }

    /// release before redelivery so the same attempt can reclaim after a failed release.
    pub(crate) async fn release_for_redelivery(&self, command: &ActionCommand) {
        self.release(command.workflow_node_run_id, command.attempt)
            .await;
    }

    /// release after settlement so only a later attempt can reclaim after a failed release.
    pub(crate) async fn release_after_settlement(&self, command: &ActionCommand) {
        self.release(command.workflow_node_run_id, command.attempt + 1)
            .await;
    }

    async fn release(&self, node_run_id: Uuid, min_reclaim_attempt: i64) {
        let Some(replica_id) = self.replica_id else {
            return;
        };
        match self
            .api_client
            .release_workflow_node_run_executor(node_run_id, replica_id, Utc::now())
            .await
        {
            Ok(_) => self.stale.clear(node_run_id).await,
            Err(err) => {
                warn!(
                    node_run_id = %node_run_id,
                    "failed to release executor lease; remembering it for reclaim: {}",
                    err
                );
                self.stale.record(node_run_id, min_reclaim_attempt).await;
            }
        }
    }
}
