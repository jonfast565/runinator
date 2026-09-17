#[allow(unused_imports)]
use super::*;

/// the container a package's code runs in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionRuntimeSpec {
    /// the runtime a manifest names, e.g. `python3.13`. resolved to an image by the worker, so a
    /// deployment can repoint a runtime without republishing every package that uses it.
    pub runtime: String,
    /// an explicit image, overriding whatever the runtime resolves to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// a script run once before the handler, for dependency installation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
}

impl FunctionRuntimeSpec {
    pub fn new(runtime: impl Into<String>) -> Self {
        Self {
            runtime: runtime.into(),
            image: None,
            setup_script: None,
        }
    }
}
