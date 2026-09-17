#[allow(unused_imports)]
use super::*;

/// optional knobs a caller can attach to a scale request; each backend interprets
/// what it can and ignores the rest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeSpec {
    /// routing labels to advertise on spun-up nodes (supervisor backend).
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// container image override (kubernetes backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// extra command-line args appended to spun-up nodes (supervisor backend).
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// backend-local group key that namespaces a node pool apart from the kind's default group, so
    /// e.g. per-org dedicated pools scale independently. `None` uses the kind's default group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}
