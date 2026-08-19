//! Hierarchical role-based access-control vocabulary.
//!
//! Roles are fixed, ordered bundles. Assignments are additive and resource grants use the
//! independent `Permission` ladder from `auth`; the authorization service selects the strongest
//! applicable value.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{Permission, PrincipalKind, ResourceType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Platform,
    Organization,
    Team,
    User,
}

impl ScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Organization => "organization",
            Self::Team => "team",
            Self::User => "user",
        }
    }

    pub fn from_str_lossy(value: &str) -> Option<Self> {
        match value {
            "platform" => Some(Self::Platform),
            "organization" => Some(Self::Organization),
            "team" => Some(Self::Team),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeRef {
    pub kind: ScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

impl ScopeRef {
    pub const PLATFORM: Self = Self {
        kind: ScopeKind::Platform,
        id: None,
    };

    pub fn new(kind: ScopeKind, id: Option<Uuid>) -> Option<Self> {
        if matches!(kind, ScopeKind::Platform) == id.is_none() {
            Some(Self { kind, id })
        } else {
            None
        }
    }
}

macro_rules! ordered_role {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }

            pub fn from_str_lossy(value: &str) -> Option<Self> {
                match value { $($wire => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

ordered_role!(PlatformRole {
    Member => "member",
    Auditor => "auditor",
    Operator => "operator",
    Admin => "admin",
});

ordered_role!(TeamRole {
    Member => "member",
    Operator => "operator",
    Admin => "admin",
    Owner => "owner",
});

ordered_role!(SystemRole {
    Waker => "waker",
    Worker => "worker",
    Agent => "agent",
    Replica => "replica",
    Engine => "engine",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Platform(PlatformRole),
    Organization(crate::orgs::OrgRole),
    Team(TeamRole),
    System(SystemRole),
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Platform(role) => role.as_str(),
            Self::Organization(role) => role.as_str(),
            Self::Team(role) => role.as_str(),
            Self::System(role) => role.as_str(),
        }
    }

    pub const fn default_permission(self) -> Permission {
        match self {
            Self::Platform(PlatformRole::Admin)
            | Self::Organization(crate::orgs::OrgRole::Owner | crate::orgs::OrgRole::Admin)
            | Self::Team(TeamRole::Owner | TeamRole::Admin) => Permission::Own,
            Self::Platform(PlatformRole::Operator)
            | Self::Organization(crate::orgs::OrgRole::Operator)
            | Self::Team(TeamRole::Operator) => Permission::Edit,
            _ => Permission::View,
        }
    }

    pub fn from_parts(kind: &str, value: &str) -> Option<Self> {
        match kind {
            "platform" => PlatformRole::from_str_lossy(value).map(Self::Platform),
            "organization" => crate::orgs::OrgRole::from_str_lossy(value).map(Self::Organization),
            "team" => TeamRole::from_str_lossy(value).map(Self::Team),
            "system" => SystemRole::from_str_lossy(value).map(Self::System),
            _ => None,
        }
    }

    pub const fn kind_str(self) -> &'static str {
        match self {
            Self::Platform(_) => "platform",
            Self::Organization(_) => "organization",
            Self::Team(_) => "team",
            Self::System(_) => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    #[serde(rename = "resource:view")]
    View,
    #[serde(rename = "resource:run")]
    Run,
    #[serde(rename = "resource:edit")]
    Edit,
    #[serde(rename = "resource:own")]
    Own,
    #[serde(rename = "members:manage")]
    MembersManage,
    #[serde(rename = "roles:manage")]
    RolesManage,
    #[serde(rename = "credentials:manage")]
    CredentialsManage,
    #[serde(rename = "secrets:read")]
    SecretsRead,
    #[serde(rename = "secrets:write")]
    SecretsWrite,
    #[serde(rename = "billing:manage")]
    BillingManage,
    #[serde(rename = "audit:read")]
    AuditRead,
    #[serde(rename = "deadletters:read")]
    DeadLettersRead,
    #[serde(rename = "nodes:operate")]
    NodesOperate,
    #[serde(rename = "schedules:manage")]
    SchedulesManage,
    #[serde(rename = "notifications:manage")]
    NotificationsManage,
    #[serde(rename = "functions:manage")]
    FunctionsManage,
    #[serde(rename = "console:use")]
    ConsoleUse,
    #[serde(rename = "catalog:manage")]
    CatalogManage,
    #[serde(rename = "agents:enroll")]
    AgentsEnroll,
    #[serde(rename = "system:engine")]
    EngineOperate,
    #[serde(rename = "system:worker")]
    WorkerOperate,
    #[serde(rename = "system:waker")]
    WakerOperate,
    #[serde(rename = "system:agent")]
    AgentOperate,
    #[serde(rename = "system:replica")]
    ReplicaOperate,
}

impl Action {
    pub const ALL: &'static [Self] = &[
        Self::View,
        Self::Run,
        Self::Edit,
        Self::Own,
        Self::MembersManage,
        Self::RolesManage,
        Self::CredentialsManage,
        Self::SecretsRead,
        Self::SecretsWrite,
        Self::BillingManage,
        Self::AuditRead,
        Self::DeadLettersRead,
        Self::NodesOperate,
        Self::SchedulesManage,
        Self::NotificationsManage,
        Self::FunctionsManage,
        Self::ConsoleUse,
        Self::CatalogManage,
        Self::AgentsEnroll,
        Self::EngineOperate,
        Self::WorkerOperate,
        Self::WakerOperate,
        Self::AgentOperate,
        Self::ReplicaOperate,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "resource:view",
            Self::Run => "resource:run",
            Self::Edit => "resource:edit",
            Self::Own => "resource:own",
            Self::MembersManage => "members:manage",
            Self::RolesManage => "roles:manage",
            Self::CredentialsManage => "credentials:manage",
            Self::SecretsRead => "secrets:read",
            Self::SecretsWrite => "secrets:write",
            Self::BillingManage => "billing:manage",
            Self::AuditRead => "audit:read",
            Self::DeadLettersRead => "deadletters:read",
            Self::NodesOperate => "nodes:operate",
            Self::SchedulesManage => "schedules:manage",
            Self::NotificationsManage => "notifications:manage",
            Self::FunctionsManage => "functions:manage",
            Self::ConsoleUse => "console:use",
            Self::CatalogManage => "catalog:manage",
            Self::AgentsEnroll => "agents:enroll",
            Self::EngineOperate => "system:engine",
            Self::WorkerOperate => "system:worker",
            Self::WakerOperate => "system:waker",
            Self::AgentOperate => "system:agent",
            Self::ReplicaOperate => "system:replica",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub principal_kind: PrincipalKind,
    pub principal_id: Uuid,
    pub scope: ScopeRef,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOwnership {
    pub resource_type: ResourceType,
    pub resource_id: Uuid,
    pub tenant: ScopeRef,
    pub owner: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub authz_version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveAccess {
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub id: Uuid,
    pub name: String,
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
