#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewAdapterRevision {
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub expected_revision: i64,
    pub kind_version: String,
    pub schema_digest: Option<String>,
    pub transport: AdapterTransport,
    pub configuration: Value,
    pub authentication: AdapterAuthentication,
    pub identity_configuration: Value,
    pub actor_id: Option<Uuid>,
}
