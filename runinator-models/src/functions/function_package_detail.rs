#[allow(unused_imports)]
use super::*;

/// a package plus everything published under it, as the API and UI read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionPackageDetail {
    #[serde(flatten)]
    pub package: FunctionPackage,
    #[serde(default)]
    pub versions: Vec<FunctionVersion>,
    #[serde(default)]
    pub aliases: Vec<FunctionAlias>,
    /// exports of the version this package's default alias resolves to.
    #[serde(default)]
    pub exports: Vec<FunctionExport>,
}
