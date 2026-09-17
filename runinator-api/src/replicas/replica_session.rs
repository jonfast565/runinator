#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct ReplicaSession {
    pub replica: ReplicaRecord,
    pub runtime_id: String,
    pub config: ReplicaServiceConfig,
}

impl ReplicaSession {
    pub fn replica_id(&self) -> Uuid {
        self.replica.replica_id
    }

    pub fn heartbeat_request(&self) -> ReplicaHeartbeatRequest {
        ReplicaHeartbeatRequest {
            runtime_id: self.runtime_id.clone(),
            display_name: self.config.display_name.clone(),
            host: self.config.host.clone(),
            port: self.config.port,
            base_path: self.config.base_path.clone(),
            attributes: self.config.attributes.clone(),
        }
    }

    pub fn offline_request(&self) -> ReplicaOfflineRequest {
        ReplicaOfflineRequest {
            runtime_id: self.runtime_id.clone(),
        }
    }
}
