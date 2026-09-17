#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderExecutionRequest {
    pub run_id: Option<Uuid>,
    pub action_name: String,
    pub action_function: String,
    #[serde(default)]
    pub parameters: Value,
    pub timeout_secs: i64,
    pub artifact_dir: String,
    pub events_jsonl_path: String,
    /// the node's resolved `.idempotent(key: ...)` value, when it declared one. providers with native
    /// idempotency (stripe-style request keys) should pass it to the upstream API so a redelivery the
    /// platform cannot absorb still lands once. `None` for non-idempotent actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Worker-resolved path for a currently fenced workspace-affined effect. The engine validates
    /// the opaque affinity before dispatch and the worker guarantees this path remains beneath its
    /// configured workspace root. Providers never receive orchestration ownership details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    /// Worker-local, effect-private credential layout. Server-side storage details never cross the
    /// provider boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_profile: Option<crate::execution_profiles::MaterializedExecutionProfile>,
    /// Plaintext credential destinations materialized by the worker immediately before provider
    /// invocation. This context is never part of the durable effect or broker payload.
    #[serde(
        default,
        skip_serializing_if = "MaterializedCredentialInjections::is_empty"
    )]
    pub credential_injections: MaterializedCredentialInjections,
}

impl ProviderExecutionRequest {
    /// path the host touches to request cooperative cancellation across the plugin ffi boundary.
    /// derived as a sibling of `events_jsonl_path` (the per-run work dir) so abi-2 plugins can locate
    /// it without a new wire field; `None` when no events path is set (unit tests bypassing a worker).
    pub fn cancel_signal_path(&self) -> Option<std::path::PathBuf> {
        if self.events_jsonl_path.is_empty() {
            return None;
        }
        std::path::Path::new(&self.events_jsonl_path)
            .parent()
            .map(|parent| parent.join("cancel.signal"))
    }
}
