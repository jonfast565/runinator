#[allow(unused_imports)]
use super::*;

#[allow(dead_code)]
#[derive(Deserialize)]
pub(super) struct AttachmentField {
    pub(super) title: Option<String>,
    pub(super) value: Option<String>,
    pub(super) short: Option<bool>,
}
