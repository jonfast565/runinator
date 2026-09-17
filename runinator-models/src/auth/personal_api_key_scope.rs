#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalApiKeyScope {
    pub org_id: Option<Uuid>,
    pub name: String,
    pub actions: Vec<Action>,
}
