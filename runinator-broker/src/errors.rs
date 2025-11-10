use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("duplicate message for key {0}")]
    Duplicate(String),
    #[error("delivery not found: {0}")]
    UnknownDelivery(uuid::Uuid),
    #[error("operation not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("internal broker error: {0}")]
    Internal(String),
}
