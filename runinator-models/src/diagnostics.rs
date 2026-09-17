//! Structured runtime diagnostics shared by producers, storage, APIs, and terminal clients.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, required_text,
};

pub const MAX_RUNTIME_LOG_BATCH: usize = 256;
pub const MAX_RUNTIME_LOG_BATCH_BYTES: usize = 64 * 1024;

fn is_zero(value: &u64) -> bool {
    *value == 0
}

mod runtime_log_record;
pub use runtime_log_record::RuntimeLogRecord;

mod runtime_log_batch;
pub use runtime_log_batch::RuntimeLogBatch;

mod runtime_log_query;
pub use runtime_log_query::RuntimeLogQuery;

mod runtime_log_page;
pub use runtime_log_page::RuntimeLogPage;
