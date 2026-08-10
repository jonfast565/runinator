use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_comm::ActionCommand;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

const EXECUTOR_LEASE_GRACE_SECONDS: i64 = 60;

#[derive(Default)]
pub(crate) struct OwnStaleLeases(Mutex<HashMap<Uuid, i64>>);

impl OwnStaleLeases {
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

/// claim this delivery's executor lease, returning whether another executor still owns it.
pub(crate) async fn claim_executor(
    api_client: &AsyncApiClient<StaticLocator>,
    stale_leases: &Arc<OwnStaleLeases>,
    replica_id: Uuid,
    command: &ActionCommand,
    timeout_seconds: i64,
) -> bool {
    let stale_before =
        Utc::now() - chrono::Duration::seconds(timeout_seconds + EXECUTOR_LEASE_GRACE_SECONDS);
    let mut held_elsewhere = false;
    match api_client
        .claim_workflow_node_run_executor(
            command.workflow_node_run_id,
            replica_id,
            Utc::now(),
            stale_before,
        )
        .await
    {
        Ok(true) => stale_leases.clear(command.workflow_node_run_id).await,
        Ok(false) => held_elsewhere = true,
        Err(err) => warn!(
            replica_id = %replica_id,
            node_run_id = %command.workflow_node_run_id,
            "failed to claim executor: {}",
            err
        ),
    }

    if held_elsewhere
        && stale_leases
            .matches(command.workflow_node_run_id, command.attempt)
            .await
        && api_client
            .release_workflow_node_run_executor(
                command.workflow_node_run_id,
                replica_id,
                Utc::now(),
            )
            .await
            .is_ok()
    {
        match api_client
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
                    stale_leases.clear(command.workflow_node_run_id).await;
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

/// release this worker's executor lease and remember failures for a safe later reclaim.
pub(crate) async fn release_executor_lease(
    api_client: &AsyncApiClient<StaticLocator>,
    stale_leases: &OwnStaleLeases,
    replica_id: Uuid,
    node_run_id: Uuid,
    min_reclaim_attempt: i64,
) {
    match api_client
        .release_workflow_node_run_executor(node_run_id, replica_id, Utc::now())
        .await
    {
        Ok(_) => stale_leases.clear(node_run_id).await,
        Err(err) => {
            warn!(
                node_run_id = %node_run_id,
                "failed to release executor lease; remembering it for reclaim: {}",
                err
            );
            stale_leases.record(node_run_id, min_reclaim_attempt).await;
        }
    }
}
