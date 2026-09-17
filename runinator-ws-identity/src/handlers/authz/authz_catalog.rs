#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct AuthzCatalog {
    pub(super) actions: &'static [Action],
    pub(super) platform_roles: [PlatformRole; 4],
    pub(super) organization_roles: [OrgRole; 4],
    pub(super) team_roles: [TeamRole; 4],
    pub(super) resource_types: [ResourceType; 10],
}
