//! the ListObjectsV2 request and response shapes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// the default page size when a caller sends no `max-keys`, matching S3.
pub const DEFAULT_MAX_KEYS: usize = 1000;

mod list_request;
pub use list_request::ListRequest;

mod bucket_summary;
pub use bucket_summary::BucketSummary;

mod object_summary;
pub use object_summary::ObjectSummary;

mod list_response;
pub use list_response::ListResponse;
