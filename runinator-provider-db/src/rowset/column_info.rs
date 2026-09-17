#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ColumnKind,
    /// the engine's own type name, kept for debugging and for callers that need precision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_type: Option<String>,
}

impl ColumnInfo {
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
        allow(dead_code)
    )]
    pub fn new(name: impl Into<String>, kind: ColumnKind) -> Self {
        Self {
            name: name.into(),
            kind,
            native_type: None,
        }
    }

    #[cfg_attr(
        not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
        allow(dead_code)
    )]
    pub fn with_native_type(mut self, native_type: impl Into<String>) -> Self {
        self.native_type = Some(native_type.into());
        self
    }
}
