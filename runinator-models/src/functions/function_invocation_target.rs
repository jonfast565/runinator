#[allow(unused_imports)]
use super::*;

/// everything a worker needs to run one export, resolved from its id.
///
/// the binding on a compiled action pins *which* code to run; this is *how* to run it. keeping the
/// handler, runtime, and limits here rather than on the binding keeps a compiled workflow small and
/// keeps one published fact in one place — a version is immutable, so resolving it is a cache hit
/// after the first call rather than a per-invocation round trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionInvocationTarget {
    pub package_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub version: i64,
    pub artifact_digest: String,
    pub runtime: FunctionRuntimeSpec,
    pub export: FunctionExport,
}
