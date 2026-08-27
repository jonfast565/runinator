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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ColumnKind,
    /// the engine's own type name, kept for debugging and for callers that need precision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_type: Option<String>,
}

impl ColumnInfo {
    #[cfg_attr(
        not(any(
            feature = "mongo",
            feature = "postgres",
            feature = "mysql",
            feature = "sqlite"
        )),
        allow(dead_code)
    )]
    pub fn new(name: impl Into<String>, kind: ColumnKind) -> Self {
        Self {
            name: name.into(),
            kind,
            native_type: None,
        }
    }

    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub fn with_native_type(mut self, native_type: impl Into<String>) -> Self {
        self.native_type = Some(native_type.into());
        self
    }
}

/// the result of a row-returning statement, holding typed json values. this is the single
/// internal representation; the two wire shapes are projections of it.
#[derive(Clone, Debug, Default)]
pub struct RowSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Value>>,
}

impl RowSet {
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub fn new(columns: Vec<ColumnInfo>, rows: Vec<Vec<Value>>) -> Self {
        Self { columns, rows }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// build a row set from documents whose keys are not known up front, unioning the keys in
    /// first-seen order so document stores and `select *` behave the same way downstream.
    #[cfg_attr(not(feature = "mongo"), allow(dead_code))]
    pub fn from_objects(documents: Vec<Map<String, Value>>) -> Self {
        let mut columns: Vec<String> = Vec::new();
        for document in &documents {
            for key in document.keys() {
                if !columns.iter().any(|existing| existing == key) {
                    columns.push(key.clone());
                }
            }
        }

        let rows = documents
            .iter()
            .map(|document| {
                columns
                    .iter()
                    .map(|column| document.get(column).cloned().unwrap_or(Value::Null))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let columns = columns
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let kind = infer_kind(rows.iter().filter_map(|row| row.get(index)));
                ColumnInfo::new(name, kind)
            })
            .collect::<Vec<_>>();

        Self { columns, rows }
    }

    /// the default wire shape: an array of objects with real json types.
    pub fn to_rows_json(&self) -> Value {
        let rows = self
            .rows
            .iter()
            .map(|row| {
                let mut object = Map::with_capacity(self.columns.len());
                for (column, value) in self.columns.iter().zip(row.iter()) {
                    object.insert(column.name.clone(), value.clone());
                }
                Value::Object(object)
            })
            .collect::<Vec<_>>();

        json!({
            "columns": self.columns,
            "rows": rows,
            "row_count": self.rows.len(),
        })
    }

    /// the flat all-strings shape, and the input the csv/excel exporters expect.
    pub fn to_table_data(&self) -> TableData {
        let headers = self
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>();
        let rows = self
            .rows
            .iter()
            .map(|row| row.iter().map(stringify).collect::<Vec<_>>())
            .collect::<Vec<_>>();

        TableData { headers, rows }
    }

    pub fn to_table_json(&self) -> Value {
        let table = self.to_table_data();
        json!({
            "headers": table.headers,
            "rows": table.rows,
            "row_count": table.rows.len(),
        })
    }
}

/// the result of a non-row-returning statement.
#[derive(Clone, Debug, Default)]
pub struct ExecOutcome {
    pub rows_affected: u64,
    pub last_insert_id: Option<Value>,
}

impl ExecOutcome {
    pub fn to_json(&self) -> Value {
        json!({
            "rows_affected": self.rows_affected,
            "last_insert_id": self.last_insert_id.clone().unwrap_or(Value::Null),
        })
    }
}

/// one entry in a script's result list. a step either returned rows or affected them.
#[cfg_attr(
    not(any(
        feature = "mongo",
        feature = "postgres",
        feature = "mysql",
        feature = "sqlite"
    )),
    allow(dead_code)
)]
#[derive(Clone, Debug)]
pub enum StepOutcome {
    Rows(RowSet),
    Affected(ExecOutcome),
}

#[derive(Clone, Debug, Serialize)]
pub struct ColumnSummary {
    pub name: String,
    #[serde(rename = "type")]
    pub native_type: String,
    pub nullable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TableInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub columns: Vec<ColumnSummary>,
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

/// pick a column kind from the values actually present, ignoring nulls. mixed types fall back
/// to `Json` because that is the only shape that can hold all of them.
#[cfg_attr(not(feature = "mongo"), allow(dead_code))]
fn infer_kind<'a>(values: impl Iterator<Item = &'a Value>) -> ColumnKind {
    let mut kind: Option<ColumnKind> = None;
    for value in values {
        let observed = match value {
            Value::Null => continue,
            Value::Bool(_) => ColumnKind::Boolean,
            Value::Number(number) if number.is_i64() || number.is_u64() => ColumnKind::Integer,
            Value::Number(_) => ColumnKind::Number,
            Value::String(_) => ColumnKind::String,
            Value::Array(_) | Value::Object(_) => ColumnKind::Json,
        };
        kind = match kind {
            None => Some(observed),
            Some(existing) if existing == observed => Some(existing),
            // integer widening to number is the one merge worth keeping precise.
            Some(ColumnKind::Integer) if observed == ColumnKind::Number => Some(ColumnKind::Number),
            Some(ColumnKind::Number) if observed == ColumnKind::Integer => Some(ColumnKind::Number),
            Some(_) => Some(ColumnKind::Json),
        };
    }
    kind.unwrap_or(ColumnKind::Null)
}
