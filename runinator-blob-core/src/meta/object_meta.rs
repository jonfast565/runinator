#[allow(unused_imports)]
use super::*;

/// an object's descriptor: everything a `HEAD` answers.
///
/// `etag` is a quoted SHA-256 digest, not the MD5 value that real S3 returns for a single-part
/// upload. Runinator uses SHA-256 for content-addressed data, so callers must not assume MD5
/// semantics here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    /// lowercase hex sha-256 of the full object.
    pub sha256: String,
    pub content_type: String,
    pub last_modified: DateTime<Utc>,
    /// `x-amz-meta-*` headers, with the prefix stripped and names lowercased.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl ObjectMeta {
    /// the quoted entity tag for this object.
    pub fn etag(&self) -> String {
        format!("\"{}\"", self.sha256)
    }
}
