#[allow(unused_imports)]
use super::*;

/// optional file output for a row-returning statement. absent means results are returned
/// in-band only.
#[derive(Clone, Debug, Deserialize)]
pub struct ExportSpec {
    pub folder: String,
    #[serde(default)]
    pub format: ExportFormat,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub file_prefix: Option<String>,
}
