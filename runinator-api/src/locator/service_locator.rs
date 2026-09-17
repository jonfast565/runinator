#[allow(unused_imports)]
use super::*;

/// Trait for types that can asynchronously resolve the base URL for the Runinator web service.
#[async_trait]
pub trait ServiceLocator: Clone + Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn wait_for_service_url(&self) -> StdResult<String, Self::Error>;
}

#[async_trait]
impl ServiceLocator for WebServiceDiscovery {
    type Error = Infallible;

    async fn wait_for_service_url(&self) -> StdResult<String, Self::Error> {
        Ok(WebServiceDiscovery::wait_for_service_url(self).await)
    }
}
