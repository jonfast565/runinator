#[allow(unused_imports)]
use super::*;

pub struct ServerResources {
    pub(super) process: ProcessResources,
    pub(super) broker: Option<Arc<dyn Broker>>,
    pub(super) blobs: Option<Arc<dyn BlobStore>>,
    pub(super) database: Option<DatabaseResource>,
}

impl ServerResources {
    pub fn builder(name: impl Into<String>) -> ServerResourcesBuilder {
        ServerResourcesBuilder::new(name)
    }

    pub fn process(&self) -> &ProcessResources {
        &self.process
    }

    pub fn broker(&self) -> Option<&Arc<dyn Broker>> {
        self.broker.as_ref()
    }

    pub fn blobs(&self) -> Option<&Arc<dyn BlobStore>> {
        self.blobs.as_ref()
    }

    pub fn database(&self) -> Option<&DatabaseResource> {
        self.database.as_ref()
    }
}
