#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkflowAction {
    pub provider: String,
    pub function: String,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: i64,
    #[serde(default)]
    pub configuration: WorkflowObject,
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    /// routing labels a worker must carry to receive this action. empty means the general pool. the
    /// reducer maps a non-empty selector to a labelled broker target and parks until a matching worker
    /// is live.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_labels: BTreeMap<String, String>,
    /// Unresolved workspace routing token. The VM resolves this beside the configuration and
    /// freezes the stable worker instance into the durable effect target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_affinity: Option<Value>,
    /// External file-backed identity selected with `@profile("name")`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_profile: Option<crate::execution_profiles::ExecutionProfileBinding>,
    /// unresolved expression naming this action's external effect, from `.idempotent(key: <expr>)`.
    /// the reducer resolves it against the run context at dispatch and stamps the result on the
    /// action command; the worker reserves that key before invoking the provider. `None` leaves the
    /// action non-idempotent, which is the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Value>,
    /// the packaged function this action invokes, pinned at compile time. `None` for an ordinary
    /// provider action, which is every action that is not a packaged-function call.
    ///
    /// this must stay a declared field: unknown top-level action keys are folded into
    /// `configuration` by the deserializer below, so a binding that were not declared would silently
    /// become a config parameter and then fail validation as an unknown one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_binding: Option<FunctionBinding>,
}

impl<'de> Deserialize<'de> for WorkflowAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawWorkflowAction {
            pub provider: String,
            pub function: String,
            #[serde(default = "default_timeout_seconds")]
            pub timeout_seconds: i64,
            #[serde(default)]
            pub configuration: Value,
            #[serde(default)]
            pub mcp_enabled: bool,
            #[serde(default)]
            pub tags: Vec<String>,
            #[serde(default)]
            pub required_labels: BTreeMap<String, String>,
            #[serde(default)]
            pub workspace_affinity: Option<Value>,
            #[serde(default)]
            pub execution_profile: Option<crate::execution_profiles::ExecutionProfileBinding>,
            #[serde(default)]
            pub idempotency_key: Option<Value>,
            #[serde(default)]
            pub function_binding: Option<FunctionBinding>,
            #[serde(flatten)]
            pub extra: Map,
        }

        let raw = RawWorkflowAction::deserialize(deserializer)?;
        if raw.extra.contains_key("metadata") {
            return Err(serde::de::Error::custom(
                "action metadata is no longer supported; use action configuration",
            ));
        }
        let configuration = merge_action_configuration(raw.configuration, raw.extra)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            provider: raw.provider,
            function: raw.function,
            timeout_seconds: raw.timeout_seconds,
            configuration,
            mcp_enabled: raw.mcp_enabled,
            tags: raw.tags,
            required_labels: raw.required_labels,
            workspace_affinity: raw.workspace_affinity,
            execution_profile: raw.execution_profile,
            idempotency_key: raw.idempotency_key,
            function_binding: raw.function_binding,
        })
    }
}
