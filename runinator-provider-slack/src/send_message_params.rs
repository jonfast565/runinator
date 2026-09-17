#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(super) struct SendMessageParams {
    pub(super) channel: String,
    pub(super) text: String,
    pub(super) team_id: Option<String>,
    pub(super) attachments: Option<Value>,
    pub(super) blocks: Option<Value>,
    pub(super) thread_ts: Option<String>,
    pub(super) mrkdwn: Option<bool>,
    pub(super) unfurl_links: Option<bool>,
    pub(super) unfurl_media: Option<bool>,
}
