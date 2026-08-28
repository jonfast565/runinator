use runinator_models::errors::SendableError;
use serde::Deserialize;
use serde_json::Value;

use crate::engine::Engine;
use crate::errors::INVALID_STATEMENT;

/// the wire shape of a SQL statement.
#[derive(Debug, Default, Deserialize)]
pub struct StatementFields {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub params: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// a statement entry in a list: either bare sql text or a full statement object.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StatementInput {
    Text(String),
    Fields(Box<StatementFields>),
}

impl StatementInput {
    pub fn into_fields(self) -> StatementFields {
        match self {
            StatementInput::Text(text) => StatementFields {
                sql: Some(text),
                ..Default::default()
            },
            StatementInput::Fields(fields) => *fields,
        }
    }
}

/// a validated SQL statement.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
    allow(dead_code)
)]
#[derive(Debug, Clone)]
pub enum StatementSpec {
    Sql {
        name: Option<String>,
        text: String,
        params: Vec<Value>,
    },
}

impl StatementSpec {
    /// the caller-supplied label for this statement, used for export filenames and step results.
    pub fn name(&self) -> Option<&str> {
        match self {
            StatementSpec::Sql { name, .. } => name.as_deref(),
        }
    }

    /// resolve the wire fields against a SQL engine.
    pub fn resolve(fields: StatementFields, engine: Engine) -> Result<Self, SendableError> {
        if let Some(name) = fields.extra.keys().next() {
            return Err(INVALID_STATEMENT.error(format!("unknown statement field '{name}'")));
        }
        let text = fields.sql.ok_or_else(|| {
            INVALID_STATEMENT.error(format!(
                "engine '{}' requires a 'sql' statement",
                engine.as_str()
            ))
        })?;
        if text.trim().is_empty() {
            return Err(INVALID_STATEMENT.error("'sql' must not be empty"));
        }

        Ok(StatementSpec::Sql {
            name: fields.name,
            text,
            params: fields.params.unwrap_or_default(),
        })
    }
}
use std::collections::BTreeMap;
