use std::{convert::Infallible, result::Result as StdResult};

use async_trait::async_trait;
use runinator_comm::discovery::WebServiceDiscovery;

/// Trait for types that can synchronously resolve the base URL for the Runinator web service.

#[cfg(test)]
#[path = "locator_tests.rs"]
mod tests;

mod service_locator;
pub use service_locator::ServiceLocator;

mod blocking_service_locator;
pub use blocking_service_locator::BlockingServiceLocator;

mod static_locator;
pub use static_locator::StaticLocator;
