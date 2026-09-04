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
pub struct ExecutionProfileCommand {
    pub argv: Vec<String>,
    #[serde(default)]
    pub interactive: bool,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileCollectionSpec {
    #[serde(default = "default_spec_version")]
    pub version: u32,
    #[serde(default)]
    pub probe: Option<ExecutionProfileCommand>,
    #[serde(default)]
    pub refresh: Option<ExecutionProfileCommand>,
    pub sources: Vec<ExecutionProfileSource>,
}

impl Default for ExecutionProfileCollectionSpec {
    fn default() -> Self {
        Self {
            version: default_spec_version(),
            probe: None,
            refresh: None,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileExposureSpec {
    #[serde(default = "default_spec_version")]
    pub version: u32,
    #[serde(default)]
    pub home_overlay: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl Default for ExecutionProfileExposureSpec {
    fn default() -> Self {
        Self {
            version: default_spec_version(),
            home_overlay: false,
            environment: BTreeMap::new(),
        }
    }
}

const fn default_spec_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfile {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub credential_scopes: Vec<String>,
    pub collection: ExecutionProfileCollectionSpec,
    pub exposure: ExecutionProfileExposureSpec,
    pub config_version: i64,
    pub config_digest: String,
    pub enabled: bool,
    pub current_revision: Option<i64>,
    pub current_digest: Option<String>,
    pub current_publisher_id: Option<Uuid>,
    pub published_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub refresh_requested_at: Option<DateTime<Utc>>,
    pub health: ExecutionProfileHealth,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

/// One durable dry-run or refresh request, claimed and completed by one desktop agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileOperation {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub config_digest: String,
    pub kind: ExecutionProfileOperationKind,
    pub state: ExecutionProfileOperationState,
    pub requested_at: DateTime<Utc>,
    pub requested_by: Option<Uuid>,
    pub claimed_by: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

/// The latest locally reported collection state from one desktop agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileAgentStatus {
    pub profile_id: Uuid,
    pub agent_id: Uuid,
    pub config_digest: String,
    pub approval: ExecutionProfileApprovalState,
    pub last_seen_at: DateTime<Utc>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// The collection status presented to profile authors alongside immutable publication metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileCollectionStatus {
    pub profile_id: Uuid,
    pub config_digest: String,
    pub publication_health: ExecutionProfileHealth,
    pub current_revision: Option<i64>,
    pub published_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub latest_operation: Option<ExecutionProfileOperation>,
    pub agents: Vec<ExecutionProfileAgentStatus>,
}

/// A desktop agent's observation after it has inspected the local approval and collection state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileAgentStatusRequest {
    pub config_digest: String,
    pub approval: ExecutionProfileApprovalState,
    #[serde(default)]
    pub last_attempt_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_success_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_error: Option<String>,
}

/// Binds an operation claim to the configuration the desktop agent approved locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileOperationClaimRequest {
    pub config_digest: String,
}

/// Completes a claimed desktop operation without altering the profile's publication availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileOperationCompleteRequest {
    pub state: ExecutionProfileOperationState,
    #[serde(default)]
    pub error: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileRevision {
    pub profile_id: Uuid,
    pub revision: i64,
    pub digest: String,
    pub size_bytes: i64,
    pub publisher_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Server-side encrypted blob URI. Never serialize it onto an HTTP response.
    #[serde(skip_serializing, default)]
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionProfileBinding {
    pub reference: ArtifactRef,
}

impl ExecutionProfileBinding {
    pub fn unresolved(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            reference: ArtifactRef::current(
                ArtifactKind::ExecutionProfile,
                Uuid::nil(),
                Some(ArtifactPath::new(None, name)),
            ),
        }
    }

    pub fn id(&self) -> Uuid {
        self.reference.id
    }

    pub fn name(&self) -> &str {
        self.reference
            .authored_path
            .as_ref()
            .map(|path| path.key.as_str())
            .unwrap_or("")
    }
}

impl<'de> Deserialize<'de> for ExecutionProfileBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Current { reference: ArtifactRef },
            Legacy { id: Uuid, name: String },
        }

        use serde::de::Error as _;

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Current { reference } => {
                if reference.kind != ArtifactKind::ExecutionProfile {
                    return Err(D::Error::custom(
                        "execution profile binding must reference an execution_profile artifact",
                    ));
                }
                Self { reference }
            }
            Wire::Legacy { id, name } => Self {
                reference: ArtifactRef::current(
                    ArtifactKind::ExecutionProfile,
                    id,
                    Some(ArtifactPath::new(None, name)),
                ),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedExecutionProfile {
    pub profile_id: Uuid,
    pub revision: i64,
    pub root: String,
    pub home: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfilePutRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub credential_scopes: Vec<String>,
    pub collection: ExecutionProfileCollectionSpec,
    #[serde(default)]
    pub exposure: ExecutionProfileExposureSpec,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfilePublishRequest {
    pub digest: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileStatusRequest {
    pub health: ExecutionProfileHealth,
    #[serde(default)]
    pub error: Option<String>,
}

impl Validate for ExecutionProfilePutRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        bounded_text("description", &self.description, LONG_TEXT_MAX)?;
        if self.credential_scopes.is_empty() {
            return Err(ValidationError::new(
                "credential_scopes",
                "must declare at least one credential scope",
            ));
        }
        let mut scopes = HashSet::new();
        for (index, scope) in self.credential_scopes.iter().enumerate() {
            required_text(
                &format!("credential_scopes[{index}]"),
                scope,
                SHORT_TEXT_MAX,
            )?;
            if !scopes.insert(scope.trim().to_ascii_lowercase()) {
                return Err(ValidationError::new(
                    format!("credential_scopes[{index}]"),
                    "must be unique ignoring case",
                ));
            }
        }
        if self.collection.version != 1 || self.exposure.version != 1 {
            return Err(ValidationError::new(
                "version",
                "only collection/exposure specification version 1 is supported",
            ));
        }
        for (label, command) in [
            ("collection.probe", self.collection.probe.as_ref()),
            ("collection.refresh", self.collection.refresh.as_ref()),
        ] {
            if let Some(command) = command {
                validate_profile_command(label, command)?;
                if label == "collection.probe" && command.interactive {
                    return Err(ValidationError::new(label, "cannot be interactive"));
                }
            }
        }
        if self.collection.sources.is_empty() {
            return Err(ValidationError::new(
                "collection.sources",
                "must contain at least one source",
            ));
        }
        let mut targets = HashSet::new();
        for (index, source) in self.collection.sources.iter().enumerate() {
            let target = match source {
                ExecutionProfileSource::File { target, .. }
                | ExecutionProfileSource::Directory { target, .. } => target,
                ExecutionProfileSource::Command { command, target } => {
                    validate_profile_command(
                        &format!("collection.sources[{index}].command"),
                        command,
                    )?;
                    if command.interactive {
                        return Err(ValidationError::new(
                            format!("collection.sources[{index}].command"),
                            "cannot be interactive",
                        ));
                    }
                    target
                }
            };
            validate_bundle_path(target).map_err(|message| {
                ValidationError::new(format!("collection.sources[{index}].target"), message)
            })?;
            if !targets.insert(target.trim().to_string()) {
                return Err(ValidationError::new(
                    format!("collection.sources[{index}].target"),
                    "duplicates another bundle target",
                ));
            }
            if let ExecutionProfileSource::Directory { glob, .. } = source
                && glob.trim().is_empty()
            {
                return Err(ValidationError::new(
                    format!("collection.sources[{index}].glob"),
                    "cannot be blank",
                ));
            }
        }
        let mut environment_names = HashSet::new();
        for (name, value) in &self.exposure.environment {
            if !is_portable_environment_name(name.trim()) {
                return Err(ValidationError::new(
                    format!("exposure.environment.{name}"),
                    "is not a portable environment variable name",
                ));
            }
            if !environment_names.insert(name.trim().to_ascii_lowercase()) {
                return Err(ValidationError::new(
                    format!("exposure.environment.{name}"),
                    "duplicates another name ignoring case",
                ));
            }
            validate_environment_template(value.trim()).map_err(|message| {
                ValidationError::new(format!("exposure.environment.{name}"), message)
            })?;
        }
        Ok(())
    }
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
    Ok(())
}

pub fn is_portable_environment_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

impl Validate for ExecutionProfileStatusRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(error) = &self.error {
            bounded_text("error", error, 512)?;
        }
        Ok(())
    }
}

impl Validate for ExecutionProfileAgentStatusRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("config_digest", &self.config_digest, SHORT_TEXT_MAX)?;
        if let Some(error) = &self.last_error {
            bounded_text("last_error", error, 512)?;
        }
        Ok(())
    }
}

impl Validate for ExecutionProfileOperationClaimRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("config_digest", &self.config_digest, SHORT_TEXT_MAX)
    }
}

impl Validate for ExecutionProfileOperationCompleteRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.state.is_active() {
            return Err(ValidationError::new(
                "state",
                "must be a terminal operation state",
            ));
        }
        if let Some(error) = &self.error {
            bounded_text("error", error, 512)?;
        }
        if self.state == ExecutionProfileOperationState::Failed && self.error.is_none() {
            return Err(ValidationError::new(
                "error",
                "is required when state is failed",
            ));
        }
        Ok(())
    }
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
