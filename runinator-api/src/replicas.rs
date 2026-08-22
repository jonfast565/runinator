use std::{sync::Arc, time::Duration};

use log::warn;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaOfflineRequest, ReplicaProviderRegistration,
    ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest,
};
use runinator_models::value::Value;
use runinator_utilities::resource_telemetry::{attributes_with_telemetry, TelemetryCollector};
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

/// An API client paired with the replica session it registered.
/// Heartbeats and provider calls use the same session.
#[derive(Clone)]
pub struct ReplicaClient<L> {
    pub api: AsyncApiClient<L>,
    pub session: ReplicaSession,
}

impl<L> ReplicaClient<L>
where
    L: ServiceLocator,
{
    /// Register a new replica session with `api` and pair them.
    pub async fn register(api: AsyncApiClient<L>, config: ReplicaServiceConfig) -> Result<Self> {
        let runtime_id = Uuid::new_v4().to_string();
        let replica = api
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
        let session = ReplicaSession {
            replica,
            runtime_id,
            config,
        };
        Ok(Self { api, session })
    }

    pub fn replica_id(&self) -> Uuid {
        self.session.replica_id()
    }

    pub async fn register_provider(
        &self,
        provider: runinator_models::providers::ProviderMetadata,
    ) -> Result<ReplicaProviderRegistration> {
        self.api
            .register_replica_provider(
                self.session.replica_id(),
                &ReplicaProviderRegistrationRequest {
                    runtime_id: self.session.runtime_id.clone(),
                    provider,
                },
            )
            .await
    }
}

impl<L> ReplicaClient<L>
where
    L: ServiceLocator + 'static,
{
    pub fn spawn_heartbeat(&self, shutdown: Arc<Notify>) -> JoinHandle<()> {
        self.spawn_heartbeat_with_telemetry(shutdown, None)
    }

    /// like [`Self::spawn_heartbeat`], but samples `collector` on every tick and folds live
    /// cpu/ram/gpu telemetry into the heartbeat attributes. pass `None` to send static attributes
    /// only.
    pub fn spawn_heartbeat_with_telemetry(
        &self,
        shutdown: Arc<Notify>,
        collector: Option<Arc<TelemetryCollector>>,
    ) -> JoinHandle<()> {
        let api_client = self.api.clone();
        let session = self.session.clone();
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
                        let mut request = session.heartbeat_request();
                        if let Some(collector) = collector.as_ref() {
                            request.attributes =
                                attributes_with_telemetry(&session.config.attributes, collector);
                        }
                        if let Err(err) = api_client
                            .heartbeat_replica(session.replica_id(), &request)
                            .await
                        {
                            warn!("Failed to heartbeat replica {}: {}", session.replica_id(), err);
                        }
                    }
                }
            }
        })
    }
}
