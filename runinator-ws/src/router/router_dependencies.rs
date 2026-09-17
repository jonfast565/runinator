#[allow(unused_imports)]
use super::*;

pub struct RouterDependencies<T> {
    pub workspace_limits: runinator_models::workspaces::WorkspaceLimits,
    pub pool: Arc<T>,
    pub events: EventSender,
    pub broker: Arc<dyn Broker>,
    pub blobs: Arc<dyn BlobStore>,
    pub provisioner: Arc<ProvisionerRegistry>,
    pub auth: AuthConfig,
    pub cors: CorsConfig,
    pub rate_limit: RateLimitConfig,
    pub circuit_breaker: CircuitBreakerConfig,
    pub overload: OverloadConfig,
}
