use std::{convert::Infallible, result::Result as StdResult};

use async_trait::async_trait;
use runinator_comm::discovery::WebServiceDiscovery;

/// Trait for types that can asynchronously resolve the base URL for the Runinator web service.
#[async_trait]
pub trait ServiceLocator: Clone + Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn wait_for_service_url(&self) -> StdResult<String, Self::Error>;
}

/// Trait for types that can synchronously resolve the base URL for the Runinator web service.
pub trait BlockingServiceLocator: Clone + Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn wait_for_service_url(&self) -> StdResult<String, Self::Error>;
}

/// Convenience locator that always returns the same base URL.
#[derive(Clone)]
pub struct StaticLocator {
    base_url: String,
}

impl StaticLocator {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl ServiceLocator for StaticLocator {
    type Error = Infallible;

    async fn wait_for_service_url(&self) -> StdResult<String, Self::Error> {
        Ok(self.base_url.clone())
    }
}

impl BlockingServiceLocator for StaticLocator {
    type Error = Infallible;

    fn wait_for_service_url(&self) -> StdResult<String, Self::Error> {
        Ok(self.base_url.clone())
    }
}

#[async_trait]
impl ServiceLocator for WebServiceDiscovery {
    type Error = Infallible;

    async fn wait_for_service_url(&self) -> StdResult<String, Self::Error> {
        Ok(WebServiceDiscovery::wait_for_service_url(self).await)
    }
}
