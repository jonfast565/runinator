#[allow(unused_imports)]
use super::*;

/// A direct connection to one of the concrete broker backends.
#[derive(Debug, Clone)]
pub struct DirectBrokerConnection {
    pub(super) config: BrokerClientConfig,
}

impl DirectBrokerConnection {
    pub fn new(config: BrokerClientConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl BrokerConnection for DirectBrokerConnection {
    fn client_config(&self) -> Result<BrokerClientConfig, BrokerBuildError> {
        Ok(self.config.clone())
    }

    fn description(&self) -> Result<String, BrokerBuildError> {
        Ok(format!(
            "direct {} @ {}",
            self.config.backend, self.config.endpoint
        ))
    }
}
