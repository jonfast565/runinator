#[allow(unused_imports)]
use super::*;

pub struct ServerResourcesBuilder {
    pub(super) name: String,
    pub(super) broker: Option<(BrokerClientConfig, BrokerConsumerProfile)>,
    pub(super) blobs: Option<BlobRequest>,
    pub(super) database: Option<DatabaseRequest>,
}

impl ServerResourcesBuilder {
    pub(super) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            broker: None,
            blobs: None,
            database: None,
        }
    }

    pub fn broker(mut self, config: BrokerClientConfig, profile: BrokerConsumerProfile) -> Self {
        self.broker = Some((config, profile));
        self
    }

    pub fn blobs(mut self, request: BlobRequest) -> Self {
        self.blobs = Some(request);
        self
    }

    pub fn database(mut self, request: DatabaseRequest) -> Self {
        self.database = Some(request);
        self
    }

    pub async fn build(self) -> Result<ServerResources, ServerBootstrapError> {
        let process = ProcessResources::start(&self.name).map_err(ServerBootstrapError::Process)?;
        let broker = match self.broker {
            Some((config, profile)) => Some(
                build_broker_client(&config, profile)
                    .await
                    .map_err(ServerBootstrapError::Broker)?,
            ),
            None => None,
        };
        let blobs = match self.blobs {
            Some(request) => {
                let store = runinator_blob::from_env()
                    .await
                    .map_err(ServerBootstrapError::Blob)?;
                if request.ensure_buckets {
                    runinator_blob::ensure_buckets(&store)
                        .await
                        .map_err(ServerBootstrapError::Blob)?;
                }
                Some(store)
            }
            None => None,
        };
        let database = match self.database {
            Some(request) => Some(resolve_database(request).await?),
            None => None,
        };
        Ok(ServerResources {
            process,
            broker,
            blobs,
            database,
        })
    }
}
