#[allow(unused_imports)]
use super::*;

/// A human-facing, stable-key location. `None` is the package's root namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactPath {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub key: String,
}

impl ArtifactPath {
    pub fn new(namespace: Option<String>, key: impl Into<String>) -> Self {
        Self {
            namespace: namespace.filter(|namespace| !namespace.is_empty()),
            key: key.into(),
        }
    }

    /// Split a dotted authoring path at its final segment. A single segment is rooted.
    pub fn from_qualified(value: impl AsRef<str>) -> Self {
        let value = value.as_ref().trim();
        match value.rsplit_once('.') {
            Some((namespace, key)) if !namespace.is_empty() && !key.is_empty() => {
                Self::new(Some(namespace.to_string()), key)
            }
            _ => Self::new(None, value),
        }
    }

    pub fn qualified(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}.{}", self.key),
            None => self.key.clone(),
        }
    }
}

impl fmt::Display for ArtifactPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.qualified())
    }
}
