use runinator_models::value::Value;
use serde::Deserialize;

#[derive(Deserialize, Default)]
pub(crate) struct EmailSendParams {
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_body: Option<String>,
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<u16>,
    #[serde(default)]
    pub smtp_user: Option<String>,
    #[serde(default)]
    pub smtp_password: Option<String>,
}

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

pub(crate) struct NotificationPayload {
    pub workflow_run_id: Option<i64>,
    pub channel: String,
    pub severity: String,
    pub title: String,
    pub body: Option<String>,
    pub target: Option<String>,
    pub metadata: Value,
}
