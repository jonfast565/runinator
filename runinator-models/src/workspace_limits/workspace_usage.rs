#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceUsage {
    /// Distinct inode lengths including sparse holes, plus canonical named result bytes.
    pub logical_bytes: u64,
    /// Directory entries, including aliases and symbolic links, excluding the root.
    pub entries: u64,
    pub results_bytes: u64,
}
