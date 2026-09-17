#[allow(unused_imports)]
use super::*;

pub struct WebserverRuntime<T> {
    pub pool: Arc<T>,
    pub notify: Arc<Notify>,
    pub port: u16,
    pub listener: Option<TcpListener>,
    pub broker: Arc<dyn Broker>,
    pub blobs: Arc<dyn runinator_blob::BlobStore>,
    pub advertisement: ReplicaAdvertisement,
    pub auth: crate::auth::AuthOptions,
    pub cors: crate::router::CorsConfig,
    pub rate_limit: crate::rate_limit::RateLimitConfig,
    pub circuit_breaker: crate::circuit_breaker::CircuitBreakerConfig,
    pub overload: crate::overload::OverloadConfig,
    pub provisioner: Option<Arc<runinator_provisioner::ProvisionerRegistry>>,
    pub run_engine: bool,
    pub max_concurrent_ingress: usize,
    pub workspace_limits: runinator_models::workspaces::WorkspaceLimits,
}
