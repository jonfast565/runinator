#[allow(unused_imports)]
use super::*;

/// a tenant. `slug` is the stable, URL/label-safe identifier used for routing labels (`org=<slug>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
