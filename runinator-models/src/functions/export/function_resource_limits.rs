#[allow(unused_imports)]
use super::*;

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
