#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewNotification {
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub source_resource_type: Option<ResourceType>,
    #[serde(default)]
    pub source_resource_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_node_id: Option<String>,
    pub channel: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    /// stable key making engine-emitted notifications idempotent: a policy that keeps matching on
    /// every scan tick collapses onto one row instead of one per tick. `None` for manual posts.
    #[serde(default)]
    pub dedupe_key: Option<String>,
}

impl Validate for NewNotification {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("channel", &self.channel, SHORT_TEXT_MAX)?;
        required_text("severity", &self.severity, SHORT_TEXT_MAX)?;
        required_text("title", &self.title, SHORT_TEXT_MAX)?;
        optional_text("body", self.body.as_deref(), LONG_TEXT_MAX)?;
        optional_text("target", self.target.as_deref(), 2 * 1024)?;
        optional_text("dedupe_key", self.dedupe_key.as_deref(), SHORT_TEXT_MAX)
    }
}
