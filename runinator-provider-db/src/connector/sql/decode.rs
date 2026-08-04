use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::{Number, Value};
use sqlx::{Column, Row, TypeInfo, ValueRef};
use sqlx::{mysql::MySqlRow, postgres::PgRow, sqlite::SqliteRow};
use uuid::Uuid;

use crate::rowset::{ColumnInfo, ColumnKind};

/// map a native type name to the coarse json shape it decodes into. the name sets differ per
/// engine but the prefixes overlap enough that one table covers all three.
pub fn kind_for(native: &str) -> ColumnKind {
    let upper = native.to_ascii_uppercase();
    match upper.as_str() {
        "BOOL" | "BOOLEAN" => ColumnKind::Boolean,
        "INT2" | "INT4" | "INT8" | "SMALLINT" | "INTEGER" | "INT" | "BIGINT" | "TINYINT"
        | "MEDIUMINT" | "SERIAL" | "BIGSERIAL" | "OID" | "INT UNSIGNED" | "BIGINT UNSIGNED"
        | "SMALLINT UNSIGNED" | "TINYINT UNSIGNED" => ColumnKind::Integer,
        "FLOAT4" | "FLOAT8" | "FLOAT" | "DOUBLE" | "REAL" | "NUMERIC" | "DECIMAL" => {
            ColumnKind::Number
        }
        "DATE" | "TIME" | "TIMETZ" | "TIMESTAMP" | "TIMESTAMPTZ" | "DATETIME" => {
            ColumnKind::Datetime
        }
        "JSON" | "JSONB" => ColumnKind::Json,
        "BYTEA" | "BLOB" | "BINARY" | "VARBINARY" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            ColumnKind::Binary
        }
        "NULL" => ColumnKind::Null,
        "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" | "UUID" | "TINYTEXT" | "MEDIUMTEXT"
        | "LONGTEXT" | "ENUM" | "SET" => ColumnKind::String,
        _ => ColumnKind::Unknown,
    }
}

/// binary columns are base64-free: hex keeps the old provider's `\x…` rendering, which is what
/// postgres itself prints and what round-trips back into a query.
fn encode_binary(bytes: Vec<u8>) -> Value {
    let mut text = String::with_capacity(bytes.len() * 2 + 2);
    text.push_str("\\x");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    Value::String(text)
}

fn number_from_f64(value: f64) -> Value {
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

macro_rules! try_decode {
    ($row:expr, $idx:expr, $ty:ty, $map:expr) => {
        if let Ok(value) = $row.try_get::<Option<$ty>, _>($idx) {
            #[allow(clippy::redundant_closure_call)]
            return match value {
                Some(inner) => ($map)(inner),
                None => Value::Null,
            };
        }
    };
}

pub fn columns_pg(row: &PgRow) -> Vec<ColumnInfo> {
    row.columns()
        .iter()
        .map(|column| {
            let native = column.type_info().name().to_string();
            ColumnInfo::new(column.name(), kind_for(&native)).with_native_type(native)
        })
        .collect()
}

pub fn columns_mysql(row: &MySqlRow) -> Vec<ColumnInfo> {
    row.columns()
        .iter()
        .map(|column| {
            let native = column.type_info().name().to_string();
            ColumnInfo::new(column.name(), kind_for(&native)).with_native_type(native)
        })
        .collect()
}

/// sqlite is dynamically typed: a column's *declared* type is `NULL` for anything that is not a
/// plain table column, so expressions and aggregates carry no static type at all. the runtime
/// storage class of the value actually returned is the reliable signal, so it wins whenever it
/// says something.
fn sqlite_runtime_type(row: &SqliteRow, idx: usize) -> Option<String> {
    let raw = row.try_get_raw(idx).ok()?;
    let name = raw.type_info().name().to_string();
    (!name.eq_ignore_ascii_case("null")).then_some(name)
}

pub fn columns_sqlite(row: &SqliteRow) -> Vec<ColumnInfo> {
    row.columns()
        .iter()
        .map(|column| {
            let declared = column.type_info().name().to_string();
            let native = match declared.eq_ignore_ascii_case("null") {
                true => sqlite_runtime_type(row, column.ordinal()).unwrap_or(declared),
                false => declared,
            };
            ColumnInfo::new(column.name(), kind_for(&native)).with_native_type(native)
        })
        .collect()
}

pub fn value_pg(row: &PgRow, idx: usize, native: &str) -> Value {
    match native.to_ascii_uppercase().as_str() {
        "BOOL" => try_decode_bool(row, idx),
        "INT2" => try_decode_i16(row, idx),
        "INT4" => try_decode_i32(row, idx),
        "INT8" => try_decode_i64(row, idx),
        "FLOAT4" => {
            try_decode!(row, idx, f32, |v: f32| number_from_f64(v as f64));
            Value::Null
        }
        "FLOAT8" => {
            try_decode!(row, idx, f64, number_from_f64);
            Value::Null
        }
        "DATE" => {
            try_decode!(row, idx, NaiveDate, |v: NaiveDate| Value::String(
                v.to_string()
            ));
            Value::Null
        }
        "TIME" => {
            try_decode!(row, idx, NaiveTime, |v: NaiveTime| Value::String(
                v.to_string()
            ));
            Value::Null
        }
        "TIMESTAMP" => {
            try_decode!(row, idx, NaiveDateTime, |v: NaiveDateTime| Value::String(
                v.and_utc().to_rfc3339()
            ));
            Value::Null
        }
        "TIMESTAMPTZ" => {
            try_decode!(row, idx, DateTime<Utc>, |v: DateTime<Utc>| Value::String(
                v.to_rfc3339()
            ));
            Value::Null
        }
        "JSON" | "JSONB" => {
            try_decode!(row, idx, Value, |v: Value| v);
            Value::Null
        }
        "UUID" => {
            try_decode!(row, idx, Uuid, |v: Uuid| Value::String(v.to_string()));
            Value::Null
        }
        "BYTEA" => {
            try_decode!(row, idx, Vec<u8>, encode_binary);
            Value::Null
        }
        _ => fallback_pg(row, idx, native),
    }
}

fn try_decode_bool(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, bool, Value::Bool);
    Value::Null
}

fn try_decode_i16(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, i16, |v: i16| Value::Number(v.into()));
    Value::Null
}

fn try_decode_i32(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, i32, |v: i32| Value::Number(v.into()));
    Value::Null
}

fn try_decode_i64(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
    Value::Null
}

/// last-resort chain for types with no explicit arm above, including `NUMERIC`, enums, and
/// arrays. an unrepresentable type is surfaced as a marker rather than failing the whole query.
fn fallback_pg(row: &PgRow, idx: usize, native: &str) -> Value {
    try_decode!(row, idx, String, Value::String);
    try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
    try_decode!(row, idx, f64, number_from_f64);
    try_decode!(row, idx, bool, Value::Bool);
    try_decode!(row, idx, Value, |v: Value| v);
    try_decode!(row, idx, Vec<u8>, encode_binary);
    Value::String(format!("<unsupported:{native}>"))
}

pub fn value_mysql(row: &MySqlRow, idx: usize, native: &str) -> Value {
    match native.to_ascii_uppercase().as_str() {
        "BOOLEAN" => {
            try_decode!(row, idx, bool, Value::Bool);
            Value::Null
        }
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" => {
            try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
            Value::Null
        }
        "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" | "INT UNSIGNED"
        | "BIGINT UNSIGNED" => {
            try_decode!(row, idx, u64, |v: u64| Value::Number(v.into()));
            Value::Null
        }
        "FLOAT" => {
            try_decode!(row, idx, f32, |v: f32| number_from_f64(v as f64));
            Value::Null
        }
        "DOUBLE" => {
            try_decode!(row, idx, f64, number_from_f64);
            Value::Null
        }
        "DATE" => {
            try_decode!(row, idx, NaiveDate, |v: NaiveDate| Value::String(
                v.to_string()
            ));
            Value::Null
        }
        "TIME" => {
            try_decode!(row, idx, NaiveTime, |v: NaiveTime| Value::String(
                v.to_string()
            ));
            Value::Null
        }
        "DATETIME" | "TIMESTAMP" => {
            try_decode!(row, idx, NaiveDateTime, |v: NaiveDateTime| Value::String(
                v.and_utc().to_rfc3339()
            ));
            Value::Null
        }
        "JSON" => {
            try_decode!(row, idx, Value, |v: Value| v);
            Value::Null
        }
        "BLOB" | "BINARY" | "VARBINARY" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            try_decode!(row, idx, Vec<u8>, encode_binary);
            Value::Null
        }
        _ => {
            try_decode!(row, idx, String, Value::String);
            try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
            try_decode!(row, idx, f64, number_from_f64);
            try_decode!(row, idx, bool, Value::Bool);
            try_decode!(row, idx, Vec<u8>, encode_binary);
            Value::String(format!("<unsupported:{native}>"))
        }
    }
}

pub fn value_sqlite(row: &SqliteRow, idx: usize, native: &str) -> Value {
    // decode against the value's runtime storage class rather than the column's declared type.
    // without this, `select count(*)` reports a declared type of `NULL` and every aggregate comes
    // back as json null; blindly falling through to the guess chain is no better, because sqlite
    // will happily coerce a text value to the integer 0.
    let runtime = sqlite_runtime_type(row, idx);
    let native = runtime.as_deref().unwrap_or(native);

    match native.to_ascii_uppercase().as_str() {
        "INTEGER" => {
            try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
            Value::Null
        }
        "REAL" => {
            try_decode!(row, idx, f64, number_from_f64);
            Value::Null
        }
        "BOOLEAN" => {
            try_decode!(row, idx, bool, Value::Bool);
            Value::Null
        }
        "BLOB" => {
            try_decode!(row, idx, Vec<u8>, encode_binary);
            Value::Null
        }
        "TEXT" => {
            try_decode!(row, idx, String, Value::String);
            Value::Null
        }
        "NULL" => Value::Null,
        _ => {
            try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
            try_decode!(row, idx, f64, number_from_f64);
            try_decode!(row, idx, String, Value::String);
            try_decode!(row, idx, bool, Value::Bool);
            try_decode!(row, idx, Vec<u8>, encode_binary);
            Value::String(format!("<unsupported:{native}>"))
        }
    }
}
