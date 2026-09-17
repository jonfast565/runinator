#[allow(unused_imports)]
use super::*;

#[derive(Deserialize, Default)]
pub struct NotificationsListQuery {
    #[serde(default)]
    pub unread: Option<bool>,
    #[serde(default)]
    pub limit: Option<i64>,
}
