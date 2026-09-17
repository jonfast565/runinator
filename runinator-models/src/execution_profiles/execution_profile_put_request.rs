#[allow(unused_imports)]
use super::*;

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
