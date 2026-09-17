#[allow(unused_imports)]
use super::*;

pub trait CredentialStore {
    fn api_key_by_prefix(
        &self,
        prefix: String,
    ) -> impl Future<Output = Option<ApiKeyRecord>> + Send;

    fn touch_api_key(&self, id: Uuid, last_used_at: i64) -> impl Future<Output = ()> + Send;

    fn user_by_id(&self, id: Uuid) -> impl Future<Output = Option<User>> + Send;

    fn session_by_id(&self, id: Uuid) -> impl Future<Output = Option<AuthSession>> + Send;

    fn service_account_by_id(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Option<ServiceAccount>> + Send;

    fn role_assignments(
        &self,
        kind: PrincipalKind,
        id: Uuid,
    ) -> impl Future<Output = Option<Vec<RoleAssignment>>> + Send;

    /// Resolve an organization selected by a token or scoped API key. Authentication must reject
    /// disabled tenants before a request reaches any resource handler.
    fn organization_by_id(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Option<runinator_models::orgs::Organization>> + Send;
}
