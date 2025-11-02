use std::error::Error;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use postgres::types::Type;
use postgres::{Client, Column, NoTls, Row};
use runinator_models::errors::{RuntimeError, SendableError};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::{DatabaseConnector, TableData};

#[derive(Clone)]
pub struct PostgresConnector {
    connection_string: String,
}

impl PostgresConnector {
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
        }
    }

    fn run_query(connection_string: String, sql: String) -> Result<TableData, SendableError> {
        let mut client = Client::connect(&connection_string, NoTls).map_err(to_sendable)?;
        let statement = client.prepare(&sql).map_err(to_sendable)?;
        let rows = client.query(&statement, &[]).map_err(to_sendable)?;
        let headers = statement
            .columns()
            .iter()
            .map(|column| column.name().to_string())
            .collect::<Vec<_>>();

        let mut data_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let mut values = Vec::with_capacity(headers.len());
            for (idx, column) in statement.columns().iter().enumerate() {
                values.push(Self::cell_to_string(&row, idx, column)?);
            }
            data_rows.push(values);
        }

        Ok(TableData {
            headers,
            rows: data_rows,
        })
    }

    fn cell_to_string(row: &Row, idx: usize, column: &Column) -> Result<String, SendableError> {
        let ty = column.type_();
        let value = match *ty {
            Type::BOOL => option_to_string(
                row.try_get::<usize, Option<bool>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::INT2 => option_to_string(
                row.try_get::<usize, Option<i16>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::INT4 => option_to_string(
                row.try_get::<usize, Option<i32>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::INT8 => option_to_string(
                row.try_get::<usize, Option<i64>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::OID => option_to_string(
                row.try_get::<usize, Option<u32>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::FLOAT4 => option_to_string(
                row.try_get::<usize, Option<f32>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::FLOAT8 => option_to_string(
                row.try_get::<usize, Option<f64>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::NUMERIC => option_to_string(
                row.try_get::<usize, Option<String>>(idx)
                    .map_err(to_sendable)?,
            ),
            Type::DATE => option_to_string(
                row.try_get::<usize, Option<NaiveDate>>(idx)
                    .map_err(to_sendable)?
                    .map(|date| date.to_string()),
            ),
            Type::TIME => option_to_string(
                row.try_get::<usize, Option<NaiveTime>>(idx)
                    .map_err(to_sendable)?
                    .map(|time| time.to_string()),
            ),
            Type::TIMESTAMP => option_to_string(
                row.try_get::<usize, Option<NaiveDateTime>>(idx)
                    .map_err(to_sendable)?
                    .map(|ts| ts.to_string()),
            ),
            Type::TIMESTAMPTZ => option_to_string(
                row.try_get::<usize, Option<DateTime<Utc>>>(idx)
                    .map_err(to_sendable)?
                    .map(|ts| ts.to_rfc3339()),
            ),
            Type::JSON | Type::JSONB => option_to_string(
                row.try_get::<usize, Option<JsonValue>>(idx)
                    .map_err(to_sendable)?
                    .map(|json| json.to_string()),
            ),
            Type::UUID => option_to_string(
                row.try_get::<usize, Option<Uuid>>(idx)
                    .map_err(to_sendable)?
                    .map(|uuid| uuid.to_string()),
            ),
            Type::BYTEA => option_to_string(
                row.try_get::<usize, Option<Vec<u8>>>(idx)
                    .map_err(to_sendable)?
                    .map(|bytes| format!("\\x{}", hex::encode(bytes))),
            ),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => row
                .try_get::<usize, Option<String>>(idx)
                .map_err(to_sendable)?
                .unwrap_or_default(),
            _ => {
                // Attempt a generic string conversion; fall back to an unsupported marker.
                match row.try_get::<usize, Option<String>>(idx) {
                    Ok(value) => value.unwrap_or_default(),
                    Err(_) => format!("<unsupported:{}>", ty.name()),
                }
            }
        };

        Ok(value)
    }
}

impl DatabaseConnector for PostgresConnector {
    fn execute_query(&self, sql: &str, timeout: Duration) -> Result<TableData, SendableError> {
        let effective_timeout = if timeout.is_zero() {
            Duration::from_secs(30)
        } else {
            timeout
        };

        let connection_string = self.connection_string.clone();
        let sql = sql.to_string();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let result = Self::run_query(connection_string, sql);
            let _ = sender.send(result);
        });

        match receiver.recv_timeout(effective_timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(Box::new(RuntimeError::new(
                "QUERY_TIMEOUT".to_string(),
                format!(
                    "PostgreSQL query timed out after {} seconds",
                    effective_timeout.as_secs()
                ),
            ))),
            Err(RecvTimeoutError::Disconnected) => Err(Box::new(RuntimeError::new(
                "QUERY_FAILED".to_string(),
                "PostgreSQL query worker exited before returning a result".to_string(),
            ))),
        }
    }
}

fn option_to_string<T>(value: Option<T>) -> String
where
    T: ToString,
{
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn to_sendable<E>(err: E) -> SendableError
where
    E: Error + Send + Sync + 'static,
{
    Box::new(err)
}
