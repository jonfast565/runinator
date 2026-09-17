use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use runinator_data_export::data_export::TableData;

/// the coarse shape of a column, derived from the driver's type info. deliberately small: it
/// describes what the json value looks like, not the engine's native type name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnKind {
    String,
    Integer,
    Number,
    Boolean,
    Datetime,
    Json,
    Binary,
    Null,
    Unknown,
}

/// one entry in a script's result list. a step either returned rows or affected them.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
    allow(dead_code)
)]
#[derive(Clone, Debug)]
pub enum StepOutcome {
    Rows(RowSet),
    Affected(ExecOutcome),
}

/// render a json value for the all-strings table shape. strings pass through unquoted and
/// null becomes empty, matching what a spreadsheet reader expects.
fn stringify(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

mod column_info;
pub use column_info::ColumnInfo;

mod row_set;
pub use row_set::RowSet;

mod exec_outcome;
pub use exec_outcome::ExecOutcome;

mod column_summary;
pub use column_summary::ColumnSummary;

mod table_info;
pub use table_info::TableInfo;
