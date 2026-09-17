use runinator_models::value::Value;
use serde::Deserialize;
use uuid::Uuid;

mod email_send_params;
pub(crate) use email_send_params::EmailSendParams;

mod notification_send_params;
pub(crate) use notification_send_params::NotificationSendParams;

mod notification_payload;
pub(crate) use notification_payload::NotificationPayload;
