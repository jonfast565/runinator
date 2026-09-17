#[allow(unused_imports)]
use super::*;

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
