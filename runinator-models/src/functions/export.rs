//! one callable entry point in a version, and the runtime it runs in.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::providers::{ParameterMetadata, ResultMetadata};

fn default_timeout_seconds() -> i64 {
    30
}

fn default_memory_mb() -> i64 {
    512
}

fn default_cpu_millis() -> i64 {
    1000
}

fn default_pids() -> i64 {
    128
}

fn default_tmp_mb() -> i64 {
    64
}

mod function_export;
pub use function_export::FunctionExport;

mod function_runtime_spec;
pub use function_runtime_spec::FunctionRuntimeSpec;

mod function_resource_limits;
pub use function_resource_limits::FunctionResourceLimits;
