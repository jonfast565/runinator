use std::{sync::Arc, time::Duration};

use log::warn;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaOfflineRequest, ReplicaProviderRegistration,
    ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest,
};
use runinator_models::value::Value;
use runinator_observability::resource_telemetry::{attributes_with_telemetry, TelemetryCollector};
use tokio::{sync::Notify, task::JoinHandle};
use uuid::Uuid;

use crate::{locator::ServiceLocator, AsyncApiClient, Result};

mod replica_service_config;
pub use replica_service_config::ReplicaServiceConfig;

mod replica_session;
pub use replica_session::ReplicaSession;

mod replica_client;
pub use replica_client::ReplicaClient;
