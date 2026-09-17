#[allow(unused_imports)]
use super::*;

/// Convenience locator that always returns the same base URL.
#[derive(Clone)]
pub struct StaticLocator {
    pub(super) base_url: String,
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
