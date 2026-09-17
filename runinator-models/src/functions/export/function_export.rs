#[allow(unused_imports)]
use super::*;

/// a named entry point with a typed signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionExport {
    pub id: Uuid,
    pub version_id: Uuid,
    pub name: String,
    /// the entry point inside the package, e.g. `src.images.resize`. its meaning is the runtime's,
    /// not ours — python reads it as a module path, node as a file plus export.
    pub handler: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Vec<ParameterMetadata>,
    #[serde(default)]
    pub output: Vec<ResultMetadata>,
    #[serde(default)]
    pub limits: FunctionResourceLimits,
}
