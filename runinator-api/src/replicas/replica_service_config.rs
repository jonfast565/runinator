#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct ReplicaServiceConfig {
    pub replica_type: ReplicaKind,
    pub instance_id: String,
    pub display_name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub base_path: Option<String>,
    pub version: Option<String>,
    pub attributes: Value,
    pub heartbeat_interval: Duration,
}

impl ReplicaServiceConfig {
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_base_path(mut self, base_path: impl Into<String>) -> Self {
        self.base_path = Some(base_path.into());
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_attributes(mut self, attributes: Value) -> Self {
        self.attributes = attributes;
        self
    }
}
