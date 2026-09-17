#[allow(unused_imports)]
use super::*;

/// a named closed enum served for the frontend's `<select>` controls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumCatalogMetadata {
    /// stable name: `gate_kind`, `match_kind`, `branch_policy`, `setting_kind`,
    /// `interrupt_source`, `resume_mode`, `concurrency_policy`.
    pub name: String,
    pub options: Vec<EnumOptionMetadata>,
}

impl EnumCatalogMetadata {
    pub fn new(name: &str, options: Vec<EnumOptionMetadata>) -> Self {
        Self {
            name: name.to_string(),
            options,
        }
    }
}
