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
}
