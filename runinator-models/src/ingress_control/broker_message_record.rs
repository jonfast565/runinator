#[allow(unused_imports)]
use super::*;

/// One engine-side observation of a message crossing a broker channel.
///
/// The trace deliberately records the broker envelope at the engine boundary. It is not a queue
/// inspector: worker-local receives remain local, while every message that enters or leaves the
/// durable engine is available to the workflow or pipeline run that owns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerMessageRecord {
    #[serde(default)]
    pub adapter_id: Option<Uuid>,
    #[serde(default)]
    pub poll_attempt_id: Option<Uuid>,
    pub id: Uuid,
    /// `effect`, `effect_result`, `wake`, `ingress`, `control`, or `agent`.
    pub channel: String,
    /// `published` when the engine wrote the message, `received` when it accepted a delivery.
    pub direction: BrokerMessageDirection,
    /// The typed broker payload carried on the channel, such as `effect_command`.
    pub message_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
}
