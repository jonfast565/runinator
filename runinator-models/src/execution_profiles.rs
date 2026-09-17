//! Provider-agnostic, file-backed execution identities.
//!
//! Profiles describe how an enrolled desktop agent collects credential material and how a worker
//! exposes one immutable publication to a provider effect. Blob locations and plaintext bytes are
//! deliberately absent from every public shape in this module.

use std::collections::BTreeMap;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::artifacts::{ArtifactKind, ArtifactPath, ArtifactRef};
use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, bounded_text, required_text,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionProfileSource {
    File {
        path: String,
        target: String,
    },
    Directory {
        path: String,
        #[serde(default = "default_glob")]
        glob: String,
        target: String,
    },
    Command {
        command: ExecutionProfileCommand,
        target: String,
    },
}

fn default_glob() -> String {
    "*".into()
}

const fn default_spec_version() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileHealth {
    #[default]
    Unpublished,
    Testing,
    Ready,
    Expiring,
    Expired,
    Error,
    Disabled,
}

/// A desktop agent's local approval for one exact execution-profile configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileApprovalState {
    Approved,
    ApprovalRequired,
}

impl ExecutionProfileApprovalState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ApprovalRequired => "approval_required",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "approved" => Self::Approved,
            _ => Self::ApprovalRequired,
        }
    }
}

/// The operator intent a desktop agent may perform for an execution profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileOperationKind {
    DryRun,
    Refresh,
}

impl ExecutionProfileOperationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DryRun => "dry_run",
            Self::Refresh => "refresh",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "refresh" => Self::Refresh,
            _ => Self::DryRun,
        }
    }
}

/// The lifecycle of one requested desktop collection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileOperationState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl ExecutionProfileOperationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }
}

impl ExecutionProfileHealth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unpublished => "unpublished",
            Self::Testing => "testing",
            Self::Ready => "ready",
            Self::Expiring => "expiring",
            Self::Expired => "expired",
            Self::Error => "error",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "testing" => Self::Testing,
            "ready" => Self::Ready,
            "expiring" => Self::Expiring,
            "expired" => Self::Expired,
            "error" => Self::Error,
            "disabled" => Self::Disabled,
            _ => Self::Unpublished,
        }
    }
}

const fn default_true() -> bool {
    true
}

fn validate_profile_command(
    path: &str,
    command: &ExecutionProfileCommand,
) -> Result<(), ValidationError> {
    if command.argv.is_empty() || command.argv.iter().any(|value| value.trim().is_empty()) {
        return Err(ValidationError::new(
            format!("{path}.argv"),
            "must contain only non-blank arguments",
        ));
    }
    let mut environment_names = HashSet::new();
    for (name, value) in &command.environment {
        if !is_portable_environment_name(name.trim()) {
            return Err(ValidationError::new(
                format!("{path}.environment.{name}"),
                "is not a portable environment variable name",
            ));
        }
        if !environment_names.insert(name.trim().to_ascii_lowercase()) {
            return Err(ValidationError::new(
                format!("{path}.environment.{name}"),
                "duplicates another name ignoring case",
            ));
        }
        if value.contains('\0') {
            return Err(ValidationError::new(
                format!("{path}.environment.{name}"),
                "cannot contain a null byte",
            ));
        }
    }
    Ok(())
}

pub fn is_portable_environment_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

pub fn validate_bundle_path(path: &str) -> Result<(), String> {
    crate::files::validate_relative_path(path)
}

pub fn validate_environment_template(value: &str) -> Result<(), String> {
    if value.starts_with('/')
        || value.as_bytes().get(1) == Some(&b':')
        || value.contains("../")
        || value.ends_with("/..")
    {
        return Err(
            "environment paths must be rooted with ${PROFILE_ROOT} or ${PROFILE_HOME}".into(),
        );
    }
    let remainder = value
        .replace("${PROFILE_ROOT}", "")
        .replace("${PROFILE_HOME}", "");
    if remainder.contains("${") {
        return Err(
            "environment values may reference only ${PROFILE_ROOT} and ${PROFILE_HOME}".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_configuration_round_trips() {
        let request = ExecutionProfilePutRequest {
            name: "github-default".into(),
            description: "GitHub CLI session".into(),
            credential_scopes: vec!["github".into(), "copilot".into()],
            collection: ExecutionProfileCollectionSpec {
                version: 1,
                probe: Some(ExecutionProfileCommand {
                    argv: vec!["gh".into(), "auth".into(), "status".into()],
                    interactive: false,
                    environment: BTreeMap::from([(
                        "GH_CONFIG_DIR".into(),
                        "~/.runinator/execution-profiles/github".into(),
                    )]),
                }),
                refresh: None,
                sources: vec![ExecutionProfileSource::Directory {
                    path: "~/.config/gh".into(),
                    glob: "*.yml".into(),
                    target: ".config/gh".into(),
                }],
            },
            exposure: ExecutionProfileExposureSpec {
                version: 1,
                home_overlay: true,
                environment: BTreeMap::from([(
                    "GH_CONFIG_DIR".into(),
                    "${PROFILE_HOME}/.config/gh".into(),
                )]),
            },
            enabled: true,
        };
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<ExecutionProfilePutRequest>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn collection_command_environment_names_are_portable() {
        let mut request = ExecutionProfilePutRequest {
            name: "fixture".into(),
            description: String::new(),
            credential_scopes: vec!["fixture".into()],
            collection: ExecutionProfileCollectionSpec {
                version: 1,
                probe: Some(ExecutionProfileCommand {
                    argv: vec!["fixture".into()],
                    interactive: false,
                    environment: BTreeMap::from([("NOT-PORTABLE".into(), "value".into())]),
                }),
                refresh: None,
                sources: vec![ExecutionProfileSource::File {
                    path: "~/.fixture".into(),
                    target: ".fixture".into(),
                }],
            },
            exposure: ExecutionProfileExposureSpec::default(),
            enabled: true,
        };

        assert!(request.validate().is_err());
        request.collection.probe.as_mut().unwrap().environment =
            BTreeMap::from([("FIXTURE_HOME".into(), "~/.fixture".into())]);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn exposure_rejects_uncontrolled_substitutions() {
        assert!(validate_environment_template("${PROFILE_HOME}/.aws").is_ok());
        assert!(validate_environment_template("${HOME}/.aws").is_err());
        assert!(validate_bundle_path("../credentials").is_err());
        assert!(validate_bundle_path(".aws/config").is_ok());
    }

    #[test]
    fn binding_reads_legacy_shape_and_emits_artifact_reference() {
        let id = Uuid::new_v4();
        let binding: ExecutionProfileBinding = serde_json::from_value(serde_json::json!({
            "id": id,
            "name": "github-default"
        }))
        .unwrap();
        assert_eq!(binding.id(), id);
        assert_eq!(binding.name(), "github-default");

        let encoded = serde_json::to_value(binding).unwrap();
        assert!(encoded.get("reference").is_some());
        assert!(encoded.get("id").is_none());
    }
}

mod execution_profile_command;
pub use execution_profile_command::ExecutionProfileCommand;

mod execution_profile_collection_spec;
pub use execution_profile_collection_spec::ExecutionProfileCollectionSpec;

mod execution_profile_exposure_spec;
pub use execution_profile_exposure_spec::ExecutionProfileExposureSpec;

mod execution_profile;
pub use execution_profile::ExecutionProfile;

mod execution_profile_operation;
pub use execution_profile_operation::ExecutionProfileOperation;

mod execution_profile_agent_status;
pub use execution_profile_agent_status::ExecutionProfileAgentStatus;

mod execution_profile_collection_status;
pub use execution_profile_collection_status::ExecutionProfileCollectionStatus;

mod execution_profile_agent_status_request;
pub use execution_profile_agent_status_request::ExecutionProfileAgentStatusRequest;

mod execution_profile_operation_claim_request;
pub use execution_profile_operation_claim_request::ExecutionProfileOperationClaimRequest;

mod execution_profile_operation_complete_request;
pub use execution_profile_operation_complete_request::ExecutionProfileOperationCompleteRequest;

mod execution_profile_revision;
pub use execution_profile_revision::ExecutionProfileRevision;

mod execution_profile_binding;
pub use execution_profile_binding::ExecutionProfileBinding;

mod materialized_execution_profile;
pub use materialized_execution_profile::MaterializedExecutionProfile;

mod execution_profile_put_request;
pub use execution_profile_put_request::ExecutionProfilePutRequest;

mod execution_profile_publish_request;
pub use execution_profile_publish_request::ExecutionProfilePublishRequest;

mod execution_profile_status_request;
pub use execution_profile_status_request::ExecutionProfileStatusRequest;
