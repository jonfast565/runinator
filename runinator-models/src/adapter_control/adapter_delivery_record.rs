#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDeliveryRecord {
    #[serde(default)]
    pub approved: bool,
    pub id: Uuid,
    pub origin: AdapterOrigin,
    pub attempt_id: Option<Uuid>,
    pub event: Option<NormalizedAdapterEvent>,
    pub state: String,
    /// Gate mode that caused a held state. Older records without this field are review-held.
    #[serde(default)]
    pub hold_mode: Option<ExternalIngressGateMode>,
    pub error: Option<String>,
    pub preview: Value,
    pub outcome: Value,
    pub received_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
