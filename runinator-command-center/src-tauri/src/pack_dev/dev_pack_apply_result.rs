#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DevPackApplyResult {
    pub path: String,
    pub files: Vec<DevPackFile>,
    pub imported: PackImportResult,
}
