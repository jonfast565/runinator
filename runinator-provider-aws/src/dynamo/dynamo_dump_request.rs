#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct DynamoDumpRequest {
    pub(super) table_name: String,
    #[serde(default)]
    pub(super) index_name: Option<String>,
    #[serde(default)]
    pub(super) key_condition_expression: Option<String>,
    #[serde(default)]
    pub(super) filter_expression: Option<String>,
    #[serde(default)]
    pub(super) projection_expression: Option<String>,
    #[serde(default)]
    pub(super) expression_attribute_values: HashMap<String, JsonValue>,
    #[serde(default)]
    pub(super) expression_attribute_names: HashMap<String, String>,
    pub(super) dump_folder: String,
    #[serde(default)]
    pub(super) file_name: Option<String>,
    #[serde(default)]
    pub(super) format: DumpFormat,
    #[serde(default)]
    pub(super) sheet_name: Option<String>,
    #[serde(default)]
    pub(super) region: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<i32>,
    #[serde(default)]
    pub(super) consistent_read: Option<bool>,
    #[serde(default)]
    pub(super) scan_index_forward: Option<bool>,
    #[serde(default)]
    pub(super) query_type: DynamoQueryType,
    #[serde(default)]
    pub(super) partiql_statement: Option<String>,
    #[serde(default)]
    pub(super) partiql_parameters: Option<Vec<JsonValue>>,
}
