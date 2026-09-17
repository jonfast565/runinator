#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewAdapterDefinition {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub kind: String,
    pub kind_version: String,
    pub schema_digest: Option<String>,
    pub transport: AdapterTransport,
    pub endpoint_identity: String,
    pub configuration: Value,
    pub authentication: AdapterAuthentication,
    pub identity_configuration: Value,
    pub actor_id: Option<Uuid>,
}
