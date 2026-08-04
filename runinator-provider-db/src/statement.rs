use runinator_models::errors::SendableError;
use serde::Deserialize;
use serde_json::Value;

use crate::engine::Engine;
use crate::errors::INVALID_STATEMENT;

/// the free-form wire shape of a single statement. one struct covers every engine; which
/// fields are meaningful is decided by [`StatementSpec::resolve`] against the engine, so a
/// caller that mixes dialects gets a clear error instead of a silently ignored field.
#[derive(Debug, Default, Deserialize)]
pub struct StatementFields {
    #[serde(default)]
    pub name: Option<String>,

    // sql engines.
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub params: Option<Vec<Value>>,

    // document engines.
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub find: Option<Value>,
    #[serde(default)]
    pub aggregate: Option<Vec<Value>>,
    #[serde(default)]
    pub insert: Option<Vec<Value>>,
    #[serde(default)]
    pub update: Option<Value>,
    #[serde(default)]
    pub delete: Option<Value>,
    #[serde(default)]
    pub command: Option<Value>,
    #[serde(default)]
    pub options: Option<DocumentOptions>,
}

/// find/aggregate shaping options for document engines.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DocumentOptions {
    #[serde(default)]
    pub projection: Option<Value>,
    #[serde(default)]
    pub sort: Option<Value>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub skip: Option<u64>,
    #[serde(default)]
    pub upsert: Option<bool>,
    /// when false an update/delete affects only the first match. defaults to true.
    #[serde(default)]
    pub multi: Option<bool>,
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

/// what a document engine should do. the document equivalent of "run a bare statement" is
/// [`DocumentCommand::Raw`], which passes a command straight through to `runCommand`.
#[derive(Debug, Clone)]
pub enum DocumentCommand {
    Find {
        filter: Value,
        options: DocumentOptions,
    },
    Aggregate {
        pipeline: Vec<Value>,
    },
    Insert {
        documents: Vec<Value>,
    },
    Update {
        filter: Value,
        update: Value,
        options: DocumentOptions,
    },
    Delete {
        filter: Value,
        options: DocumentOptions,
    },
    Raw {
        command: Value,
    },
}

impl DocumentCommand {
    /// whether this command is expected to return documents rather than counts.
    pub fn returns_documents(&self) -> bool {
        matches!(
            self,
            DocumentCommand::Find { .. }
                | DocumentCommand::Aggregate { .. }
                | DocumentCommand::Raw { .. }
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            DocumentCommand::Find { .. } => "find",
            DocumentCommand::Aggregate { .. } => "aggregate",
            DocumentCommand::Insert { .. } => "insert",
            DocumentCommand::Update { .. } => "update",
            DocumentCommand::Delete { .. } => "delete",
            DocumentCommand::Raw { .. } => "command",
        }
    }
}

/// an engine-resolved statement. keeping sql and document dialects in separate variants means
/// a connector never has to guess which half of a union struct is populated.
#[derive(Debug, Clone)]
pub enum StatementSpec {
    Sql {
        name: Option<String>,
        text: String,
        params: Vec<Value>,
    },
    Document {
        name: Option<String>,
        collection: String,
        command: DocumentCommand,
    },
}

impl StatementSpec {
    /// the caller-supplied label for this statement, used for export filenames and step results.
    pub fn name(&self) -> Option<&str> {
        match self {
            StatementSpec::Sql { name, .. } => name.as_deref(),
            StatementSpec::Document { name, .. } => name.as_deref(),
        }
    }

    /// resolve free-form fields against an engine, rejecting cross-dialect mixes.
    pub fn resolve(fields: StatementFields, engine: Engine) -> Result<Self, SendableError> {
        if engine.is_document_store() {
            return Self::resolve_document(fields, engine);
        }
        Self::resolve_sql(fields, engine)
    }

    fn resolve_sql(fields: StatementFields, engine: Engine) -> Result<Self, SendableError> {
        let document_fields = [
            ("collection", fields.collection.is_some()),
            ("find", fields.find.is_some()),
            ("aggregate", fields.aggregate.is_some()),
            ("insert", fields.insert.is_some()),
            ("update", fields.update.is_some()),
            ("delete", fields.delete.is_some()),
            ("command", fields.command.is_some()),
        ];
        if let Some((name, _)) = document_fields.iter().find(|(_, present)| *present) {
            return Err(INVALID_STATEMENT.error(format!(
                "'{name}' is a document-store field and is not supported by engine '{}'; use 'sql' instead",
                engine.as_str()
            )));
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

    fn resolve_document(fields: StatementFields, engine: Engine) -> Result<Self, SendableError> {
        if fields.sql.is_some() {
            return Err(INVALID_STATEMENT.error(format!(
                "engine '{}' does not accept 'sql'; use 'find', 'aggregate', 'insert', 'update', 'delete', or 'command'",
                engine.as_str()
            )));
        }

        let options = fields.options.unwrap_or_default();
        let selected = [
            fields.find.is_some(),
            fields.aggregate.is_some(),
            fields.insert.is_some(),
            fields.update.is_some(),
            fields.delete.is_some(),
            fields.command.is_some(),
        ]
        .iter()
        .filter(|present| **present)
        .count();

        if selected == 0 {
            return Err(INVALID_STATEMENT.error(format!(
                "engine '{}' requires one of 'find', 'aggregate', 'insert', 'update', 'delete', or 'command'",
                engine.as_str()
            )));
        }
        if selected > 1 {
            return Err(
                INVALID_STATEMENT.error("exactly one document operation may be set per statement")
            );
        }

        // a raw command targets the database, not a collection, so it is the one operation
        // that does not require `collection`.
        if let Some(command) = fields.command {
            return Ok(StatementSpec::Document {
                name: fields.name,
                collection: fields.collection.unwrap_or_default(),
                command: DocumentCommand::Raw { command },
            });
        }

        let collection = fields.collection.ok_or_else(|| {
            INVALID_STATEMENT.error("'collection' is required for document operations")
        })?;

        let command = if let Some(filter) = fields.find {
            DocumentCommand::Find { filter, options }
        } else if let Some(pipeline) = fields.aggregate {
            DocumentCommand::Aggregate { pipeline }
        } else if let Some(documents) = fields.insert {
            DocumentCommand::Insert { documents }
        } else if let Some(update) = fields.update {
            let (filter, update) = split_update(update)?;
            DocumentCommand::Update {
                filter,
                update,
                options,
            }
        } else if let Some(filter) = fields.delete {
            DocumentCommand::Delete { filter, options }
        } else {
            unreachable!("selected == 1 and command was handled above")
        };

        Ok(StatementSpec::Document {
            name: fields.name,
            collection,
            command,
        })
    }
}

/// an update carries both halves: `{ "filter": {...}, "set": {...} }` or an explicit
/// `{ "filter": {...}, "update": {"$inc": {...}} }` for operators other than `$set`.
fn split_update(value: Value) -> Result<(Value, Value), SendableError> {
    let Value::Object(mut map) = value else {
        return Err(INVALID_STATEMENT
            .error("'update' must be an object with a 'filter' and a 'set' or 'update'"));
    };

    let filter = map
        .remove("filter")
        .unwrap_or_else(|| Value::Object(Default::default()));

    if let Some(update) = map.remove("update") {
        return Ok((filter, update));
    }
    if let Some(set) = map.remove("set") {
        return Ok((filter, serde_json::json!({ "$set": set })));
    }

    Err(INVALID_STATEMENT.error("'update' requires either a 'set' or an 'update' document"))
}
