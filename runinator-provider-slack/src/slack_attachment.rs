#[allow(unused_imports)]
use super::*;

#[allow(dead_code)]
#[derive(Deserialize)]
pub(super) struct SlackAttachment {
    pub(super) fallback: Option<String>,
    pub(super) color: Option<String>,
    pub(super) pretext: Option<String>,
    pub(super) author_name: Option<String>,
    pub(super) author_link: Option<String>,
    pub(super) author_icon: Option<String>,
    pub(super) title: Option<String>,
    pub(super) title_link: Option<String>,
    pub(super) text: Option<String>,
    pub(super) fields: Option<Vec<AttachmentField>>,
    pub(super) image_url: Option<String>,
    pub(super) thumb_url: Option<String>,
    pub(super) footer: Option<String>,
    pub(super) footer_icon: Option<String>,
    pub(super) ts: Option<i64>,
    pub(super) mrkdwn_in: Option<Vec<String>>,
}
