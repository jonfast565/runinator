//! organizations, membership, quotas, usage samples, and resource groups.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use uuid::Uuid;

use runinator_models::{
    billing::{OrgQuota, OrgResourceGroup, UsageSample},
    errors::SendableError,
    orgs::{OrgMembership, OrgRole, Organization},
};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::reducer_store::ReducerStore;

/// Core persistence operations for Runinator.
/// Organizations, membership, quotas, usage samples, and resource groups.
pub trait OrgStore: Send + Sync + 'static {
    // ---- organizations (tenants) + memberships ----

    /// Create an organization.
    fn create_org(
        &self,
        name: String,
        slug: String,
    ) -> impl Future<Output = Result<Organization, SendableError>> + Send;

    /// Fetch an org by its unique slug.
    fn fetch_org_by_slug(
        &self,
        slug: String,
    ) -> impl Future<Output = Result<Option<Organization>, SendableError>> + Send;

    /// List every org (platform-admin view).
    fn list_orgs(&self) -> impl Future<Output = Result<Vec<Organization>, SendableError>> + Send;

    /// Rename or (dis|en)able an org.
    fn update_org(
        &self,
        id: Uuid,
        name: Option<String>,
        disabled: Option<bool>,
    ) -> impl Future<Output = Result<Organization, SendableError>> + Send;

    /// Delete an org and its memberships.
    fn delete_org(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Add/update a user's membership in an org (idempotent on the (org, user) pair).
    fn add_org_member(
        &self,
        org_id: Uuid,
        user_id: Uuid,
        role: OrgRole,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Remove a user from an org.
    fn remove_org_member(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// A single membership record, or `None` when the user is not in the org.
    fn fetch_org_membership(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrgMembership>, SendableError>> + Send;

    /// Every membership in an org (its member roster).
    fn list_org_members(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrgMembership>, SendableError>> + Send;

    /// The orgs a user belongs to, paired with their role in each.
    fn list_user_orgs(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<(Organization, OrgRole)>, SendableError>> + Send;

    // ---- billing: per-org quotas + usage ledger ----

    /// An org's quota, or `None` when none is set (unbounded).
    fn fetch_org_quota(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrgQuota>, SendableError>> + Send;

    /// Create or replace an org's quota.
    fn upsert_org_quota(
        &self,
        quota: OrgQuota,
    ) -> impl Future<Output = Result<OrgQuota, SendableError>> + Send;

    /// Append a usage sample to the ledger.
    fn insert_usage_sample(
        &self,
        sample: UsageSample,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Usage samples for an org since a unix-seconds cutoff, ordered by time.
    fn fetch_usage_samples(
        &self,
        org_id: Uuid,
        since: i64,
    ) -> impl Future<Output = Result<Vec<UsageSample>, SendableError>> + Send;

    /// Create or replace an org's dedicated allocation for a (backend, kind).
    fn upsert_org_resource_group(
        &self,
        group: OrgResourceGroup,
    ) -> impl Future<Output = Result<OrgResourceGroup, SendableError>> + Send;

    /// Every org's dedicated allocations (for aggregate reconcile + usage sampling).
    fn list_all_resource_groups(
        &self,
    ) -> impl Future<Output = Result<Vec<OrgResourceGroup>, SendableError>> + Send;
}
