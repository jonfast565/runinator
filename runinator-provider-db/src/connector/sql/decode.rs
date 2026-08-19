#[cfg(any(feature = "postgres", feature = "mysql"))]
use std::str::FromStr;

#[cfg(any(feature = "postgres", feature = "mysql"))]
use bigdecimal::BigDecimal;
#[cfg(feature = "postgres")]
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{Number, Value};
use sqlx::{Column, Row, TypeInfo};
#[cfg(feature = "mysql")]
use sqlx::mysql::MySqlRow;
#[cfg(feature = "postgres")]
use sqlx::postgres::PgRow;
#[cfg(feature = "sqlite")]
use sqlx::{ValueRef, sqlite::SqliteRow};
#[cfg(feature = "postgres")]
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

/// render an exact DECIMAL/NUMERIC without silently rounding it. a json number is an f64 once it
/// leaves here, which covers money and ratios but not the 38- and 65-digit precisions both engines
/// allow, so the value is only emitted as a number when it survives the round trip unchanged.
/// anything wider keeps every digit as a string rather than losing some without saying so.
#[cfg(any(feature = "postgres", feature = "mysql"))]
pub(crate) fn decimal_to_json(value: &BigDecimal) -> Value {
    let text = value.to_string();
    if let Ok(float) = text.parse::<f64>()
        && let Some(number) = Number::from_f64(float)
        && BigDecimal::from_str(&number.to_string())
            .is_ok_and(|round_tripped| &round_tripped == value)
    {
        return Value::Number(number);
    }
    // the raw string keeps the column's declared scale, so `decimal(10,2)` stays `2.50`.
    Value::String(text)
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

#[cfg(feature = "postgres")]
pub fn columns_pg(row: &PgRow) -> Vec<ColumnInfo> {
    row.columns()
        .iter()
        .map(|column| {
            let native = column.type_info().name().to_string();
            ColumnInfo::new(column.name(), kind_for(&native)).with_native_type(native)
        })
        .collect()
}

/// mysql-family names the shared table deliberately leaves out, because the same name means
/// something else on another engine: postgres `BIT` is a bit *string*, not an integer.
#[cfg(feature = "mysql")]
fn mysql_kind_for(native: &str) -> Option<ColumnKind> {
    match native.to_ascii_uppercase().as_str() {
        "BIT" | "YEAR" => Some(ColumnKind::Integer),
        _ => None,
    }
}

#[cfg(feature = "mysql")]
pub fn columns_mysql(row: &MySqlRow) -> Vec<ColumnInfo> {
    row.columns()
        .iter()
        .map(|column| {
            let native = column.type_info().name().to_string();
            let idx = column.ordinal();
            // two mysql-family names are ambiguous, and the reported kind has to agree with what
            // `value_mysql` will actually produce for the same cell. both checks sample the first
            // row, which is the only row this function is given.
            let kind = match mysql_kind_for(&native) {
                Some(kind) => kind,
                None => match kind_for(&native) {
                    ColumnKind::Binary if mysql_blob_is_json(row, idx) => ColumnKind::Json,
                    ColumnKind::Boolean if !mysql_boolean_is_flag(row, idx) => ColumnKind::Integer,
                    other => other,
                },
            };
            ColumnInfo::new(column.name(), kind).with_native_type(native)
        })
        .collect()
}

/// sqlite is dynamically typed: a column's *declared* type is `NULL` for anything that is not a
/// plain table column, so expressions and aggregates carry no static type at all. the runtime
/// storage class of the value actually returned is the reliable signal, so it wins whenever it
/// says something.
#[cfg(feature = "sqlite")]
fn sqlite_runtime_type(row: &SqliteRow, idx: usize) -> Option<String> {
    let raw = row.try_get_raw(idx).ok()?;
    let name = raw.type_info().name().to_string();
    (!name.eq_ignore_ascii_case("null")).then_some(name)
}

#[cfg(feature = "sqlite")]
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

#[cfg(feature = "postgres")]
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
        "NUMERIC" | "DECIMAL" => {
            try_decode!(row, idx, BigDecimal, |v: BigDecimal| decimal_to_json(&v));
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

#[cfg(feature = "postgres")]
fn try_decode_bool(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, bool, Value::Bool);
    Value::Null
}

#[cfg(feature = "postgres")]
fn try_decode_i16(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, i16, |v: i16| Value::Number(v.into()));
    Value::Null
}

#[cfg(feature = "postgres")]
fn try_decode_i32(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, i32, |v: i32| Value::Number(v.into()));
    Value::Null
}

#[cfg(feature = "postgres")]
fn try_decode_i64(row: &PgRow, idx: usize) -> Value {
    try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
    Value::Null
}

/// last-resort chain for types with no explicit arm above, including `NUMERIC`, enums, and
/// arrays. an unrepresentable type is surfaced as a marker rather than failing the whole query.
#[cfg(feature = "postgres")]
fn fallback_pg(row: &PgRow, idx: usize, native: &str) -> Value {
    try_decode!(row, idx, String, Value::String);
    try_decode!(row, idx, i64, |v: i64| Value::Number(v.into()));
    try_decode!(row, idx, f64, number_from_f64);
    try_decode!(row, idx, bool, Value::Bool);
    try_decode!(row, idx, Value, |v: Value| v);
    try_decode!(row, idx, Vec<u8>, encode_binary);
    Value::String(format!("<unsupported:{native}>"))
}

/// parse bytes that are a json *document*. the leading-byte gate keeps this from running a full
/// parse over every binary blob, and restricting the result to objects and arrays is what makes it
/// safe to apply to a column that might really be binary: `x'313233'` is a far more plausible blob
/// than a json column holding a bare `123`, and misreading real binary is the worse failure.
#[cfg(feature = "mysql")]
fn json_document(bytes: &[u8]) -> Option<Value> {
    let first = bytes.iter().find(|byte| !byte.is_ascii_whitespace())?;
    if !matches!(first, b'{' | b'[') {
        return None;
    }
    match serde_json::from_slice(bytes) {
        Ok(value @ (Value::Object(_) | Value::Array(_))) => Some(value),
        _ => None,
    }
}

#[cfg(feature = "mysql")]
fn mysql_blob_is_json(row: &MySqlRow, idx: usize) -> bool {
    matches!(
        row.try_get::<Option<Vec<u8>>, _>(idx),
        Ok(Some(bytes)) if json_document(&bytes).is_some()
    )
}

/// a real `boolean` only ever holds 0 or 1; anything else came from a `tinyint(1)` being used as
/// the small integer it actually is.
#[cfg(feature = "mysql")]
fn mysql_boolean_is_flag(row: &MySqlRow, idx: usize) -> bool {
    matches!(
        row.try_get::<Option<i64>, _>(idx),
        Ok(None) | Ok(Some(0)) | Ok(Some(1))
    )
}

/// mysql has no distinct boolean type: both `boolean` and `tinyint(1)` are reported as BOOLEAN by
/// both engines, and sqlx's `bool` decode maps every non-zero byte to `true`, so a `tinyint(1)`
/// holding 4 would arrive as `true`. read the integer and narrow to a json bool only for the 0/1 a
/// real boolean can hold.
#[cfg(feature = "mysql")]
fn decode_mysql_boolean(row: &MySqlRow, idx: usize) -> Value {
    if let Ok(value) = row.try_get::<Option<i64>, _>(idx) {
        return match value {
            None => Value::Null,
            Some(0) => Value::Bool(false),
            Some(1) => Value::Bool(true),
            Some(other) => Value::Number(other.into()),
        };
    }
    try_decode!(row, idx, bool, Value::Bool);
    Value::Null
}

/// mysql 8 reports a `json` column as JSON, but mariadb implements json as `longtext` plus a check
/// constraint and reports BLOB — the same name it gives a real blob, with nothing in the type info
/// to tell them apart. the payload is the only signal left.
#[cfg(feature = "mysql")]
fn decode_mysql_blob(row: &MySqlRow, idx: usize) -> Value {
    match row.try_get::<Option<Vec<u8>>, _>(idx) {
        Ok(Some(bytes)) => match json_document(&bytes) {
            Some(document) => document,
            None => encode_binary(bytes),
        },
        Ok(None) => Value::Null,
        Err(_) => Value::Null,
    }
}

#[cfg(feature = "mysql")]
pub fn value_mysql(row: &MySqlRow, idx: usize, native: &str) -> Value {
    match native.to_ascii_uppercase().as_str() {
        "BOOLEAN" => decode_mysql_boolean(row, idx),
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
        // mysql calls the wire type NEWDECIMAL; both engines report `decimal`/`numeric` columns
        // through it.
        "DECIMAL" | "NEWDECIMAL" | "NUMERIC" => {
            try_decode!(row, idx, BigDecimal, |v: BigDecimal| decimal_to_json(&v));
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
            decode_mysql_blob(row, idx)
        }
        // bit arrives as a big-endian byte string. sqlx will hand it over as `bool`, which the
        // fallback chain below would happily take, flattening `bit(8)` = 170 to `true`. u64 covers
        // every width mysql allows.
        "BIT" => {
            try_decode!(row, idx, u64, |v: u64| Value::Number(v.into()));
            Value::Null
        }
        // year decodes into none of the types the fallback chain tries, so without an arm it
        // renders as `<unsupported:YEAR>`.
        "YEAR" => {
            try_decode!(row, idx, u16, |v: u16| Value::Number(v.into()));
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

#[cfg(feature = "sqlite")]
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
