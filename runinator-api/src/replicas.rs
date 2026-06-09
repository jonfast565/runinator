use std::{sync::Arc, time::Duration};

use log::warn;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaOfflineRequest, ReplicaProviderRegistration,
    ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest,
};
use runinator_models::value::Value;
use tokio::{sync::Notify, task::JoinHandle};
use uuid::Uuid;

use crate::{locator::ServiceLocator, AsyncApiClient, Result};

#[derive(Debug, Clone)]
pub struct ReplicaServiceConfig {
    pub replica_type: ReplicaKind,
    pub instance_id: String,
    pub display_name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub base_path: Option<String>,
    pub version: Option<String>,
    pub attributes: Value,
    pub heartbeat_interval: Duration,
}

impl ReplicaServiceConfig {
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_base_path(mut self, base_path: impl Into<String>) -> Self {
        self.base_path = Some(base_path.into());
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_attributes(mut self, attributes: Value) -> Self {
        self.attributes = attributes;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ReplicaSession {
    pub replica: ReplicaRecord,
    pub runtime_id: String,
    pub config: ReplicaServiceConfig,
}

impl ReplicaSession {
    pub fn replica_id(&self) -> Uuid {
        self.replica.replica_id
    }

    pub fn heartbeat_request(&self) -> ReplicaHeartbeatRequest {
        ReplicaHeartbeatRequest {
            runtime_id: self.runtime_id.clone(),
            display_name: self.config.display_name.clone(),
            host: self.config.host.clone(),
            port: self.config.port,
            base_path: self.config.base_path.clone(),
            attributes: self.config.attributes.clone(),
        }
    }

    pub fn offline_request(&self) -> ReplicaOfflineRequest {
        ReplicaOfflineRequest {
            runtime_id: self.runtime_id.clone(),
        }
    }
}

pub async fn register_replica_session<L>(
    api_client: &AsyncApiClient<L>,
    config: ReplicaServiceConfig,
) -> Result<ReplicaSession>
where
    L: ServiceLocator,
{
    let runtime_id = Uuid::new_v4().to_string();
    let replica = api_client
        .register_replica(&ReplicaRegistrationRequest {
            replica_type: config.replica_type,
            instance_id: config.instance_id.clone(),
            runtime_id: runtime_id.clone(),
            display_name: config.display_name.clone(),
            host: config.host.clone(),
            port: config.port,
            base_path: config.base_path.clone(),
            version: config.version.clone(),
            attributes: config.attributes.clone(),
        })
        .await?;
    Ok(ReplicaSession {
        replica,
        runtime_id,
        config,
    })
}

pub fn spawn_replica_heartbeat<L>(
    api_client: AsyncApiClient<L>,
    session: ReplicaSession,
    shutdown: Arc<Notify>,
) -> JoinHandle<()>
where
    L: ServiceLocator + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(session.config.heartbeat_interval);
        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    if let Err(err) = api_client
                        .mark_replica_offline(session.replica_id(), &session.offline_request())
                        .await
                    {
                        warn!("Failed to mark replica {} offline: {}", session.replica_id(), err);
                    }
                    return;
                }
                _ = ticker.tick() => {
                    if let Err(err) = api_client
                        .heartbeat_replica(session.replica_id(), &session.heartbeat_request())
                        .await
                    {
                        warn!("Failed to heartbeat replica {}: {}", session.replica_id(), err);
                    }
                }
            }
        }
    })
}

pub async fn register_replica_provider<L>(
    api_client: &AsyncApiClient<L>,
    session: &ReplicaSession,
    provider: runinator_models::providers::ProviderMetadata,
) -> Result<ReplicaProviderRegistration>
where
    L: ServiceLocator,
{
    api_client
        .register_replica_provider(
            session.replica_id(),
            &ReplicaProviderRegistrationRequest {
                runtime_id: session.runtime_id.clone(),
                provider,
            },
        )
        .await
}
