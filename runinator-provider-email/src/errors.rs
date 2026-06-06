use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::EmailProvider;

// numbered error dictionary for the email provider (email + notification actions).
pub(crate) const INVALID_PARAMS: ErrorDescriptor = ErrorDescriptor::new(
    "EMAIL001",
    "email.invalid_params",
    "Invalid email parameters",
);
pub(crate) const INVALID: ErrorDescriptor =
    ErrorDescriptor::new("EMAIL002", "email.invalid", "Invalid email request");
pub(crate) const SMTP_CONFIG: ErrorDescriptor = ErrorDescriptor::new(
    "EMAIL003",
    "email.smtp_config",
    "Invalid SMTP configuration",
);
pub(crate) const SMTP_SEND: ErrorDescriptor =
    ErrorDescriptor::new("EMAIL004", "email.smtp_send", "Failed to send email");
pub(crate) const RUNTIME: ErrorDescriptor =
    ErrorDescriptor::new("EMAIL005", "email.runtime", "Failed to start async runtime");
pub(crate) const UNKNOWN_ACTION: ErrorDescriptor =
    ErrorDescriptor::new("EMAIL006", "email.unknown_action", "Unknown action");
pub(crate) const NOTIFICATION_INVALID_PARAMS: ErrorDescriptor = ErrorDescriptor::new(
    "EMAIL007",
    "notification.invalid_params",
    "Invalid notification parameters",
);
pub(crate) const NOTIFICATION_SERVICE_URL: ErrorDescriptor = ErrorDescriptor::new(
    "EMAIL008",
    "notification.service_url",
    "Missing notification service URL",
);
pub(crate) const NOTIFICATION_POST: ErrorDescriptor = ErrorDescriptor::new(
    "EMAIL009",
    "notification.post",
    "Failed to post notification",
);
pub(crate) const NOTIFICATION_RESPONSE: ErrorDescriptor = ErrorDescriptor::new(
    "EMAIL010",
    "notification.response",
    "Invalid notification response",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    INVALID,
    SMTP_CONFIG,
    SMTP_SEND,
    RUNTIME,
    UNKNOWN_ACTION,
    NOTIFICATION_INVALID_PARAMS,
    NOTIFICATION_SERVICE_URL,
    NOTIFICATION_POST,
    NOTIFICATION_RESPONSE,
];

impl ProviderErrors for EmailProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
