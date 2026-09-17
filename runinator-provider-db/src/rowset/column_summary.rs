#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct ColumnSummary {
    pub name: String,
    #[serde(rename = "type")]
    pub native_type: String,
    pub nullable: bool,
}
