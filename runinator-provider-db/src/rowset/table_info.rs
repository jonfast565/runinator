#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct TableInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub columns: Vec<ColumnSummary>,
}
