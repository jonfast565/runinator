#[allow(unused_imports)]
use super::*;

/// one page of a listing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResponse {
    pub objects: Vec<ObjectSummary>,
    /// prefixes rolled up by the delimiter, if one was supplied.
    #[serde(default)]
    pub common_prefixes: Vec<String>,
    pub is_truncated: bool,
    /// the token that fetches the next page; `None` when this page is the last.
    #[serde(default)]
    pub next_continuation_token: Option<String>,
}
