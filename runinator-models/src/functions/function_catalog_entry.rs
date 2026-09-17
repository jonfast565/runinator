#[allow(unused_imports)]
use super::*;

/// one export as the rest of the system sees it: everything needed to type a call, pin it, and
/// dispatch it, flattened out of the package/version/export nesting.
///
/// this is the compile-time and catalog view. it is deliberately denormalised — a compiler running
/// offline against a pack's own sources has no database to join through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCatalogEntry {
    pub package_id: Uuid,
    pub package_name: String,
    /// the namespace qualifying the package name, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub version_id: Uuid,
    /// the package version this entry describes, monotonic per package.
    pub version: i64,
    pub export_id: Uuid,
    pub export_name: String,
    pub artifact_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Vec<ParameterMetadata>,
    #[serde(default)]
    pub output: Vec<ResultMetadata>,
    /// the aliases currently resolving to this entry's version, e.g. `["production", "latest"]`.
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl FunctionCatalogEntry {
    /// the provider name this export is authored under, e.g. `functions.image_tools`.
    ///
    /// the namespace is folded in so two orgs' packages of the same name stay distinguishable in a
    /// catalog that is keyed by provider name.
    pub fn provider_name(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!(
                "{FUNCTIONS_NAMESPACE_PREFIX}{namespace}.{}",
                self.package_name
            ),
            None => format!("{FUNCTIONS_NAMESPACE_PREFIX}{}", self.package_name),
        }
    }

    /// the action metadata a compiler and the editor type this call against.
    pub fn action_metadata(&self) -> ActionMetadata {
        ActionMetadata {
            function_name: self.export_name.clone(),
            description: self.description.clone(),
            parameters: self.input.clone(),
            results: self.output.clone(),
            // packaged code runs a container; it is never reducer-evaluable in process.
            pure: false,
            delivery_semantics: Default::default(),
            agent: None,
            authentication: None,
            credential_scopes: None,
        }
    }

    /// the binding a compiled workflow records so it keeps calling exactly this version.
    pub fn binding(&self) -> FunctionBinding {
        FunctionBinding {
            package_id: self.package_id,
            package_name: self.package_name.clone(),
            namespace: self.namespace.clone(),
            version_id: self.version_id,
            version: self.version,
            export_id: self.export_id,
            export_name: self.export_name.clone(),
            artifact_digest: self.artifact_digest.clone(),
        }
    }
}
