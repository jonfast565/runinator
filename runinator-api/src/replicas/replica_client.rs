#[allow(unused_imports)]
use super::*;

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
                replica_id: None,
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
