//! Hierarchical role assignments and resource ownership.

use std::future::Future;

use runinator_models::{
    auth::{Grant, PrincipalKind, ResourceType},
    errors::SendableError,
    rbac::{ResourceOwnership, Role, RoleAssignment, ScopeRef, ServiceAccount},
};
use uuid::Uuid;

pub trait RbacStore: Send + Sync + 'static {
    fn create_service_account(
        &self,
        name: String,
        created_by: Option<Uuid>,
    ) -> impl Future<Output = Result<ServiceAccount, SendableError>> + Send;

    fn fetch_service_account(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<ServiceAccount>, SendableError>> + Send;

    fn list_service_accounts(
        &self,
    ) -> impl Future<Output = Result<Vec<ServiceAccount>, SendableError>> + Send;

    fn set_service_account_disabled(
        &self,
        id: Uuid,
        disabled: bool,
    ) -> impl Future<Output = Result<ServiceAccount, SendableError>> + Send;

    fn upsert_role_assignment(
        &self,
        principal_kind: PrincipalKind,
        principal_id: Uuid,
        scope: ScopeRef,
        role: Role,
        created_by: Option<Uuid>,
    ) -> impl Future<Output = Result<RoleAssignment, SendableError>> + Send;

    fn delete_role_assignment(
        &self,
        principal_kind: PrincipalKind,
        principal_id: Uuid,
        scope: ScopeRef,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn list_principal_role_assignments(
        &self,
        principal_kind: PrincipalKind,
        principal_id: Uuid,
    ) -> impl Future<Output = Result<Vec<RoleAssignment>, SendableError>> + Send;

    fn list_scope_role_assignments(
        &self,
        scope: ScopeRef,
    ) -> impl Future<Output = Result<Vec<RoleAssignment>, SendableError>> + Send;

    fn put_resource_ownership(
        &self,
        ownership: ResourceOwnership,
    ) -> impl Future<Output = Result<ResourceOwnership, SendableError>> + Send;

    fn fetch_resource_ownership(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
    ) -> impl Future<Output = Result<Option<ResourceOwnership>, SendableError>> + Send;

    fn list_resource_ownerships(
        &self,
        resource_type: ResourceType,
    ) -> impl Future<Output = Result<Vec<ResourceOwnership>, SendableError>> + Send;

    fn transfer_resource_ownership(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
        owner: ScopeRef,
    ) -> impl Future<Output = Result<ResourceOwnership, SendableError>> + Send;

    /// Revoke only when the grant belongs to the authorized parent resource.
    fn revoke_scoped_grant(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
        grant_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn list_effective_resource_grants(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
        principal_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;
}
