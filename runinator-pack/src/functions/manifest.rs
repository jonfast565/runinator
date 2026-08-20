//! the `runinator-function.json` manifest: what a package declares about itself.

use std::fs;
use std::path::Path;

use runinator_models::functions::{
    DEFAULT_ALIAS, FunctionRuntimeSpec, NewFunctionExport, NewFunctionPackage, NewFunctionVersion,
};
use runinator_models::value::Value;
use serde::{Deserialize, Serialize};

use crate::errors::{PackError, Result};

/// the manifest file a package directory is identified by.
///
/// json rather than toml so the pack crate keeps its dependency set and so schemas deserialize
/// straight into [`runinator_models::providers::ParameterMetadata`] — the same shape every other
/// manifest in the repo uses (`.rexrapm` is json too).
pub const MANIFEST_FILE: &str = "runinator-function.json";

/// a package's declared identity, runtime, exports, and what to archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionManifest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub runtime: FunctionRuntimeSpec,
    pub exports: Vec<NewFunctionExport>,
    /// the alias to move onto the published version. defaults to `latest`; an explicit `null`
    /// publishes without moving anything, which is how a release is staged before promotion.
    #[serde(default = "default_alias")]
    pub alias: Option<String>,
    /// extra path patterns to leave out of the archive, on top of [`super::archive::DEFAULT_EXCLUDES`].
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_alias() -> Option<String> {
    Some(DEFAULT_ALIAS.to_string())
}

impl FunctionManifest {
    /// read and validate the manifest in a package directory.
    pub fn load(directory: &Path) -> Result<Self> {
        let path = directory.join(MANIFEST_FILE);
        if !path.is_file() {
            return Err(PackError::source(format!(
                "{} is not a function package: no {MANIFEST_FILE}",
                directory.display()
            )));
        }
        let text = fs::read_to_string(&path)?;
        let manifest: Self = serde_json::from_str(&text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// reject a manifest that would publish something unaddressable or uncallable.
    pub fn validate(&self) -> Result<()> {
        validate_ident("package name", &self.name)?;
        if let Some(namespace) = &self.namespace {
            validate_ident("namespace", namespace)?;
        }
        if self.runtime.runtime.trim().is_empty() {
            return Err(PackError::source("runtime must name a runtime"));
        }
        if self.exports.is_empty() {
            return Err(PackError::source(
                "a package must declare at least one export",
            ));
        }
        let mut seen = Vec::with_capacity(self.exports.len());
        for export in &self.exports {
            validate_ident("export name", &export.name)?;
            if export.handler.trim().is_empty() {
                return Err(PackError::source(format!(
                    "export '{}' has no handler",
                    export.name
                )));
            }
            if seen.contains(&export.name) {
                return Err(PackError::source(format!(
                    "export '{}' is declared twice",
                    export.name
                )));
            }
            seen.push(export.name.clone());
        }
        if let Some(alias) = &self.alias {
            validate_ident("alias", alias)?;
        }
        Ok(())
    }

    /// the publish request this manifest describes, for an archive with the given digest.
    pub fn publish_request(&self, artifact_digest: &str) -> NewFunctionVersion {
        NewFunctionVersion {
            package: NewFunctionPackage {
                name: self.name.clone(),
                namespace: self.namespace.clone(),
                description: self.description.clone(),
                // the server stamps the caller's org; a manifest never names one, so a pack cannot
                // publish into an org it does not belong to.
                org_id: None,
            },
            artifact_digest: artifact_digest.to_string(),
            manifest: self.as_value(),
            runtime: self.runtime.clone(),
            exports: self.exports.clone(),
            alias: self.alias.clone(),
        }
    }

    /// the manifest as stored verbatim on the version.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self)
            .map(Value::from)
            .unwrap_or(Value::Null)
    }

    /// the exports, sorted by name — the order the catalog and the cli present them in.
    pub fn sorted_exports(&self) -> Vec<NewFunctionExport> {
        let mut exports = self.exports.clone();
        exports.sort_by(|left, right| left.name.cmp(&right.name));
        exports
    }
}

// package, namespace, export, and alias names all become part of a dotted call path
// (`functions.<namespace>.<package>.<export>`), so they have to survive being split on `.`.
fn validate_ident(what: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(PackError::source(format!("{what} must not be empty")));
    }
    if value.len() > 64 {
        return Err(PackError::source(format!(
            "{what} '{value}' is longer than 64 characters"
        )));
    }
    let head_is_alpha = value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic());
    if !head_is_alpha {
        return Err(PackError::source(format!(
            "{what} '{value}' must start with a letter"
        )));
    }
    let body_is_clean = value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-');
    if !body_is_clean {
        return Err(PackError::source(format!(
            "{what} '{value}' may only contain letters, digits, '_', and '-'"
        )));
    }
    Ok(())
}
