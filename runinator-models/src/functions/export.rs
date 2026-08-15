//! one callable entry point in a version, and the runtime it runs in.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::providers::{ParameterMetadata, ResultMetadata};

/// a named entry point with a typed signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionExport {
    pub id: Uuid,
    pub version_id: Uuid,
    pub name: String,
    /// the entry point inside the package, e.g. `src.images.resize`. its meaning is the runtime's,
    /// not ours — python reads it as a module path, node as a file plus export.
    pub handler: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Vec<ParameterMetadata>,
    #[serde(default)]
    pub output: Vec<ResultMetadata>,
    #[serde(default)]
    pub limits: FunctionResourceLimits,
}

/// the container a package's code runs in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionRuntimeSpec {
    /// the runtime a manifest names, e.g. `python3.13`. resolved to an image by the worker, so a
    /// deployment can repoint a runtime without republishing every package that uses it.
    pub runtime: String,
    /// an explicit image, overriding whatever the runtime resolves to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// a script run once before the handler, for dependency installation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
}

impl FunctionRuntimeSpec {
    pub fn new(runtime: impl Into<String>) -> Self {
        Self {
            runtime: runtime.into(),
            image: None,
            setup_script: None,
        }
    }
}

/// what one invocation may consume.
///
/// every field has a default because a manifest that omits them must still produce a bounded
/// sandbox — an unset limit means "the default", never "unlimited".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionResourceLimits {
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: i64,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: i64,
    #[serde(default = "default_cpu_millis")]
    pub cpu_millis: i64,
    /// process cap, which is what stops a fork bomb from taking the worker with it.
    #[serde(default = "default_pids")]
    pub pids: i64,
    /// writable scratch space, mounted as a tmpfs so it cannot outlive the invocation.
    #[serde(default = "default_tmp_mb")]
    pub tmp_mb: i64,
    /// network access. off by default: most packaged code is a pure transformation, and an opt-in
    /// keeps a compromised package from reaching the cluster it runs in.
    #[serde(default)]
    pub network: bool,
}

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

impl Default for FunctionResourceLimits {
    fn default() -> Self {
        Self {
            timeout_seconds: default_timeout_seconds(),
            memory_mb: default_memory_mb(),
            cpu_millis: default_cpu_millis(),
            pids: default_pids(),
            tmp_mb: default_tmp_mb(),
            network: false,
        }
    }
}
