#[allow(unused_imports)]
use super::*;

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
