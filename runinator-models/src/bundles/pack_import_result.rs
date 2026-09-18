#[allow(unused_imports)]
use super::*;

/// the result of importing a compiled pack zip at `/packs/import`: the imported workflow bundle,
/// the imported (redacted) secret bundle, and the pipelines that were upserted.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct PackImportResult {
    #[serde(default)]
    pub workflows: crate::workflows::WorkflowBundle,
    #[serde(default)]
    #[serde(alias = "secrets")]
    pub settings: SettingsBundle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_profiles: Vec<ExecutionProfileImportResult>,
    #[serde(default)]
    pub pipelines: Vec<crate::pipelines::Pipeline>,
    /// Setting slots the pack's workflows reference that this organization has not provisioned,
    /// as `"<kind> <scope>/<name>"`.
    ///
    /// A portable pack may legitimately reference settings provisioned after it, so these do not
    /// fail the apply. Reporting them here is what stops the first notice from being a worker
    /// failure in phase one of a billable mission: no pack in the repository declares a `secret`
    /// slot, and `sdlc-missions` alone consumes three tokens across eighteen call sites.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_settings: Vec<String>,
}
