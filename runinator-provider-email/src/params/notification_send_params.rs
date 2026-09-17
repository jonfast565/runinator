#[allow(unused_imports)]
use super::*;

#[derive(Deserialize, Default)]
pub(crate) struct NotificationSendParams {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}
