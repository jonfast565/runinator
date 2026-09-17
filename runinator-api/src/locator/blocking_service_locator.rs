#[allow(unused_imports)]
use super::*;

pub trait BlockingServiceLocator: Clone + Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn wait_for_service_url(&self) -> StdResult<String, Self::Error>;
}
