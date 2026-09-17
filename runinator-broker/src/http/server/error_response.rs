#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct ErrorResponse {
    pub(super) code: &'static str,
    pub(super) message: String,
}

impl ErrorResponse {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(super) fn duplicate(key: String) -> Self {
        Self::new("duplicate", key)
    }
    pub(super) fn unknown_delivery(id: uuid::Uuid) -> Self {
        Self::new("unknown_delivery", id.to_string())
    }
}
