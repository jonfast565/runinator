#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterPollAttempt {
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub adapter_revision: i64,
    pub dry_run: bool,
    pub state: String,
    pub result: Value,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
}
