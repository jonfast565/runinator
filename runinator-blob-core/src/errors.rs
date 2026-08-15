//! the numbered error vocabulary every blob participant shares.

use runinator_models::errors::{EngineErrors, ErrorDescriptor};
use thiserror::Error;

/// what can go wrong reaching a blob store. the variants are the ones a caller can act on
/// differently: a missing object is a 404, a rejected key is a 400, a signature failure is a 403,
/// and everything else is transport or backend trouble worth retrying.
#[derive(Debug, Error)]
pub enum BlobError {
    #[error("BLOB001 - object not found: {0}")]
    NotFound(String),
    #[error("BLOB002 - bucket not found: {0}")]
    NoSuchBucket(String),
    #[error("BLOB003 - invalid object key: {0}")]
    InvalidKey(String),
    #[error("BLOB004 - object already exists: {0}")]
    AlreadyExists(String),
    #[error("BLOB005 - digest mismatch: expected {expected}, computed {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("BLOB006 - unsatisfiable range for object of {size} bytes: {range}")]
    RangeNotSatisfiable { range: String, size: u64 },
    #[error("BLOB007 - request signature rejected: {0}")]
    Unauthorized(String),
    #[error("BLOB008 - blob storage io failure: {0}")]
    Io(String),
    #[error("BLOB009 - blob transport failure: {0}")]
    Transport(String),
    #[error("BLOB010 - bucket is not empty: {0}")]
    BucketNotEmpty(String),
    #[error("BLOB011 - unknown multipart upload: {0}")]
    NoSuchUpload(String),
    #[error("BLOB012 - malformed blob request: {0}")]
    BadRequest(String),
}

impl BlobError {
    /// the s3 `<Code>` a client expects for this failure. clients branch on these strings, so they
    /// are part of the wire contract rather than cosmetic.
    pub fn s3_code(&self) -> &'static str {
        match self {
            BlobError::NotFound(_) => "NoSuchKey",
            BlobError::NoSuchBucket(_) => "NoSuchBucket",
            BlobError::InvalidKey(_) => "InvalidArgument",
            BlobError::AlreadyExists(_) => "PreconditionFailed",
            BlobError::DigestMismatch { .. } => "BadDigest",
            BlobError::RangeNotSatisfiable { .. } => "InvalidRange",
            BlobError::Unauthorized(_) => "AccessDenied",
            BlobError::Io(_) => "InternalError",
            BlobError::Transport(_) => "InternalError",
            BlobError::BucketNotEmpty(_) => "BucketNotEmpty",
            BlobError::NoSuchUpload(_) => "NoSuchUpload",
            BlobError::BadRequest(_) => "InvalidRequest",
        }
    }

    /// the http status this failure maps to, shared by the server's error rendering and the
    /// client's status-to-error mapping so a round trip preserves the variant.
    pub fn http_status(&self) -> u16 {
        match self {
            BlobError::NotFound(_) | BlobError::NoSuchBucket(_) | BlobError::NoSuchUpload(_) => 404,
            BlobError::InvalidKey(_) | BlobError::BadRequest(_) => 400,
            BlobError::AlreadyExists(_) => 412,
            BlobError::DigestMismatch { .. } => 400,
            BlobError::RangeNotSatisfiable { .. } => 416,
            BlobError::Unauthorized(_) => 403,
            BlobError::BucketNotEmpty(_) => 409,
            BlobError::Io(_) | BlobError::Transport(_) => 500,
        }
    }

    /// rebuild a variant from a status plus the s3 code the server sent, so a client call fails the
    /// same way a local call would.
    pub fn from_s3_code(code: &str, status: u16, detail: String) -> Self {
        match code {
            "NoSuchKey" => BlobError::NotFound(detail),
            "NoSuchBucket" => BlobError::NoSuchBucket(detail),
            "InvalidArgument" => BlobError::InvalidKey(detail),
            "PreconditionFailed" => BlobError::AlreadyExists(detail),
            "BadDigest" => BlobError::DigestMismatch {
                expected: detail.clone(),
                actual: detail,
            },
            "InvalidRange" => BlobError::RangeNotSatisfiable {
                range: detail,
                size: 0,
            },
            "AccessDenied" => BlobError::Unauthorized(detail),
            "BucketNotEmpty" => BlobError::BucketNotEmpty(detail),
            "NoSuchUpload" => BlobError::NoSuchUpload(detail),
            _ if status == 404 => BlobError::NotFound(detail),
            _ if status == 403 => BlobError::Unauthorized(detail),
            _ => BlobError::Transport(detail),
        }
    }
}

pub const NOT_FOUND: ErrorDescriptor =
    ErrorDescriptor::new("BLOB001", "blob.not_found", "Object not found");
pub const NO_SUCH_BUCKET: ErrorDescriptor =
    ErrorDescriptor::new("BLOB002", "blob.no_such_bucket", "Bucket not found");
pub const INVALID_KEY: ErrorDescriptor =
    ErrorDescriptor::new("BLOB003", "blob.invalid_key", "Invalid object key");
pub const ALREADY_EXISTS: ErrorDescriptor =
    ErrorDescriptor::new("BLOB004", "blob.already_exists", "Object already exists");
pub const DIGEST_MISMATCH: ErrorDescriptor =
    ErrorDescriptor::new("BLOB005", "blob.digest_mismatch", "Digest mismatch");
pub const RANGE_NOT_SATISFIABLE: ErrorDescriptor = ErrorDescriptor::new(
    "BLOB006",
    "blob.range_not_satisfiable",
    "Unsatisfiable byte range",
);
pub const UNAUTHORIZED: ErrorDescriptor =
    ErrorDescriptor::new("BLOB007", "blob.unauthorized", "Request signature rejected");
pub const IO: ErrorDescriptor =
    ErrorDescriptor::new("BLOB008", "blob.io", "Blob storage io failure");
pub const TRANSPORT: ErrorDescriptor =
    ErrorDescriptor::new("BLOB009", "blob.transport", "Blob transport failure");
pub const BUCKET_NOT_EMPTY: ErrorDescriptor =
    ErrorDescriptor::new("BLOB010", "blob.bucket_not_empty", "Bucket is not empty");
pub const NO_SUCH_UPLOAD: ErrorDescriptor =
    ErrorDescriptor::new("BLOB011", "blob.no_such_upload", "Unknown multipart upload");
pub const BAD_REQUEST: ErrorDescriptor =
    ErrorDescriptor::new("BLOB012", "blob.bad_request", "Malformed blob request");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    NOT_FOUND,
    NO_SUCH_BUCKET,
    INVALID_KEY,
    ALREADY_EXISTS,
    DIGEST_MISMATCH,
    RANGE_NOT_SATISFIABLE,
    UNAUTHORIZED,
    IO,
    TRANSPORT,
    BUCKET_NOT_EMPTY,
    NO_SUCH_UPLOAD,
    BAD_REQUEST,
];

impl EngineErrors for BlobError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
