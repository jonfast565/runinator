#[allow(unused_imports)]
use super::*;

/// a single editable field on a form. wraps the shared `ParameterMetadata` schema with an
/// optional widget hint so the frontend can pick a richer control (`cron`, `duration`,
/// `node_ref`, `json`, `expression`, ...).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiField {
    #[serde(flatten)]
    pub param: ParameterMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
}

impl UiField {
    pub fn new(param: ParameterMetadata) -> Self {
        Self {
            param,
            widget: None,
        }
    }

    pub fn with_widget(mut self, widget: impl Into<String>) -> Self {
        self.widget = Some(widget.into());
        self
    }
}

impl From<ParameterMetadata> for UiField {
    fn from(param: ParameterMetadata) -> Self {
        Self::new(param)
    }
}
