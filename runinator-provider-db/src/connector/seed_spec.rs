#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Deserialize)]
pub struct SeedSpec {
    /// the SQL table to insert into.
    pub table: String,
    pub rows: Vec<Map<String, Value>>,
    #[serde(default)]
    pub on_conflict: OnConflict,
}
