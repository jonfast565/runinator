use thiserror::Error;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    #[error("WORKSPACE004 - Workspace storage I/O failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("WORKSPACE005 - Invalid workspace JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("WORKSPACE003 - Workspace content or reference is invalid: {0}")]
    Invalid(String),
    #[error("WORKSPACE006 - Corrupt workspace storage: {0}")]
    Corrupt(String),
    #[error("WORKSPACE007 - Workspace object not found: {0}")]
    NotFound(String),
    #[error("WORKSPACE008 - Workspace path already exists: {0}")]
    Exists(String),
    #[error("WORKSPACE002 - Workspace version or checkout is no longer current")]
    Conflict,
    #[error("WORKSPACE009 - Workspace storage is busy: {0}")]
    Busy(String),
    #[error("WORKSPACE010 - Workspace cache budget exhausted")]
    CacheFull,
    #[error("WORKSPACE011 - Workspace storage synchronization failed")]
    Poisoned,
    #[error("WORKSPACE012 - Workspace publication outcome is uncertain: {0}")]
    CommitUncertain(String),
}
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
pub(crate) fn corrupt(message: impl Into<String>) -> Error {
    Error::Corrupt(message.into())
}
