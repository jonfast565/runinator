#[allow(unused_imports)]
use super::*;

/// the result of a row-returning statement, holding typed json values. this is the single
/// internal representation; the two wire shapes are projections of it.
#[derive(Clone, Debug, Default)]
pub struct RowSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Value>>,
}

impl RowSet {
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
        allow(dead_code)
    )]
    pub fn new(columns: Vec<ColumnInfo>, rows: Vec<Vec<Value>>) -> Self {
        Self { columns, rows }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
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
