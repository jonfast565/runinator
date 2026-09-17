// Authentication and identity wire types. Credentials carry identity only; authorization is
// resolved from live RBAC state.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::rbac::{Action, PlatformRole, RoleAssignment, SystemRole};
use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, http_url, identifier, optional_email,
    optional_text, required_text, string_map,
};

/// the local-password identity provider tag. future SSO providers use `"OpenID Connect (OIDC):<issuer>"`.
pub const PROVIDER_LOCAL: &str = "local";

// ---- resource-based authorization ----

/// the permission ladder for a resource grant. higher variants subsume lower ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    View,
    Run,
    Edit,
    Own,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::View => "view",
            Permission::Run => "run",
            Permission::Edit => "edit",
            Permission::Own => "own",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "view" => Some(Permission::View),
            "run" => Some(Permission::Run),
            "edit" => Some(Permission::Edit),
            "own" => Some(Permission::Own),
            _ => None,
        }
    }

    /// true when this permission is at least as strong as `required`.
    pub fn allows(self, required: Permission) -> bool {
        self >= required
    }
}

/// the kind of resource a grant targets. workflows are the primary owned object; their runs and
/// sub-resources inherit the parent workflow's permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Workflow,
    Pipeline,
    FunctionPackage,
    ConsoleSession,
    Workspace,
    Setting,
    ExecutionProfile,
    OrchestrationAdapter,
    LibraryFile,
    NotificationPolicy,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Workflow => "workflow",
            ResourceType::Pipeline => "pipeline",
            ResourceType::FunctionPackage => "function_package",
            ResourceType::ConsoleSession => "console_session",
            ResourceType::Workspace => "workspace",
            ResourceType::Setting => "setting",
            ResourceType::ExecutionProfile => "execution_profile",
            ResourceType::OrchestrationAdapter => "orchestration_adapter",
            ResourceType::LibraryFile => "library_file",
            ResourceType::NotificationPolicy => "notification_policy",
        }
    }

    pub fn from_str_lossy(value: &str) -> Option<Self> {
        match value {
            "workflow" => Some(Self::Workflow),
            "pipeline" => Some(Self::Pipeline),
            "function_package" => Some(Self::FunctionPackage),
            "console_session" => Some(Self::ConsoleSession),
            "workspace" => Some(Self::Workspace),
            "setting" => Some(Self::Setting),
            "execution_profile" => Some(Self::ExecutionProfile),
            "orchestration_adapter" => Some(Self::OrchestrationAdapter),
            "library_file" => Some(Self::LibraryFile),
            "notification_policy" => Some(Self::NotificationPolicy),
            _ => None,
        }
    }
}

/// whether a grant is held by a user or a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalType {
    User,
    Team,
}

impl PrincipalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrincipalType::User => "user",
            PrincipalType::Team => "team",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "user" => Some(PrincipalType::User),
            "team" => Some(PrincipalType::Team),
            _ => None,
        }
    }
}

/// how a request was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    #[default]
    User,
    Service,
}

impl PrincipalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Service => "service",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "user" => Some(Self::User),
            "service" => Some(Self::Service),
            _ => None,
        }
    }
}

// ---- request/response DTOs ----

fn deserialize_platform_role_patch<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<PlatformRole>>, D::Error> {
    Option::<PlatformRole>::deserialize(deserializer).map(Some)
}

fn validate_human_platform_role(role: Option<PlatformRole>) -> Result<(), ValidationError> {
    if role.is_some_and(|role| role != PlatformRole::Admin) {
        return Err(ValidationError::new(
            "platform_role",
            "human platform access must be admin or null",
        ));
    }
    Ok(())
}

mod grant;
pub use grant::Grant;

mod team;
pub use team::Team;

mod create_team_request;
pub use create_team_request::CreateTeamRequest;

mod update_team_request;
pub use update_team_request::UpdateTeamRequest;

mod add_team_member_request;
pub use add_team_member_request::AddTeamMemberRequest;

mod create_grant_request;
pub use create_grant_request::CreateGrantRequest;

mod user;
pub use user::User;

mod user_identity;
pub use user_identity::UserIdentity;

mod user_identity_put_request;
pub use user_identity_put_request::UserIdentityPutRequest;

mod user_view;
pub use user_view::UserView;

mod local_credential;
pub use local_credential::LocalCredential;

mod api_key;
pub use api_key::ApiKey;

mod api_key_record;
pub use api_key_record::ApiKeyRecord;

mod auth_session;
pub use auth_session::AuthSession;

mod auth_session_summary;
pub use auth_session_summary::AuthSessionSummary;

mod personal_api_key_scope;
pub use personal_api_key_scope::PersonalApiKeyScope;

mod claims;
pub use claims::Claims;

mod replica_claims;
pub use replica_claims::ReplicaClaims;

mod auth_context;
pub use auth_context::AuthContext;

mod login_request;
pub use login_request::LoginRequest;

mod auth_config_response;
pub use auth_config_response::AuthConfigResponse;

mod login_response;
pub use login_response::LoginResponse;

mod refresh_request;
pub use refresh_request::RefreshRequest;

mod create_user_request;
pub use create_user_request::CreateUserRequest;

mod update_user_request;
pub use update_user_request::UpdateUserRequest;

mod update_current_user_request;
pub use update_current_user_request::UpdateCurrentUserRequest;

mod change_password_request;
pub use change_password_request::ChangePasswordRequest;

mod create_personal_api_key_request;
pub use create_personal_api_key_request::CreatePersonalApiKeyRequest;

mod create_api_key_request;
pub use create_api_key_request::CreateApiKeyRequest;

mod update_api_key_request;
pub use update_api_key_request::UpdateApiKeyRequest;

mod create_api_key_response;
pub use create_api_key_response::CreateApiKeyResponse;

mod agent_enrollment_token;
pub use agent_enrollment_token::AgentEnrollmentToken;

mod agent_enrollment_token_record;
pub use agent_enrollment_token_record::AgentEnrollmentTokenRecord;

mod agent_machine_enrollment;
pub use agent_machine_enrollment::AgentMachineEnrollment;

mod create_agent_enrollment_token_request;
pub use create_agent_enrollment_token_request::CreateAgentEnrollmentTokenRequest;

mod create_agent_enrollment_token_response;
pub use create_agent_enrollment_token_response::CreateAgentEnrollmentTokenResponse;

mod agent_enrollment_request_body;
pub use agent_enrollment_request_body::AgentEnrollmentRequestBody;

mod enroll_agent_request;
pub use enroll_agent_request::EnrollAgentRequest;

mod enroll_agent_response;
pub use enroll_agent_response::EnrollAgentResponse;
