use std::sync::Arc;
use std::time::Duration;

use mongodb::bson::{Bson, Document};
use mongodb::options::{ClientOptions, IndexOptions};
use mongodb::{Client, Collection, Database, IndexModel};
use runinator_models::errors::SendableError;
use serde_json::{Map, Value};
use tokio::runtime::Runtime;

use crate::connector::timeout::with_timeout;
use crate::connector::{DatabaseConnector, ProvisionSpec, SeedSpec};
use crate::errors::{
    CONNECTION_FAILED, DATABASE_MISSING, INVALID_STATEMENT, STATEMENT_FAILED, UNSUPPORTED_ENGINE,
};
use crate::rowset::{ColumnSummary, ExecOutcome, RowSet, StepOutcome, TableInfo};
use crate::statement::{DocumentCommand, DocumentOptions, StatementSpec};

pub struct MongoConnector {
    uri: String,
    database: String,
    runtime: Arc<Runtime>,
}

impl MongoConnector {
    pub fn new(connection: &str, runtime: Arc<Runtime>) -> Result<Self, SendableError> {
        if connection.trim().is_empty() {
            return Err(CONNECTION_FAILED.error("'connection' must not be empty"));
        }

        // The database must come from the URI. Every operation below is scoped to one database,
        // and mongo has no notion of a "current" database on a bare connection.
        let options = runtime
            .block_on(async { ClientOptions::parse(connection).await })
            .map_err(|err| CONNECTION_FAILED.error(err.to_string()))?;
        let database = options.default_database.clone().ok_or_else(|| {
            DATABASE_MISSING
                .error("'connection' must include a database, e.g. mongodb://host:27017/mydb")
        })?;

        Ok(Self {
            uri: connection.to_string(),
            database,
            runtime,
        })
    }

    async fn database(&self) -> Result<Database, SendableError> {
        let client = Client::with_uri_str(&self.uri)
            .await
            .map_err(|err| CONNECTION_FAILED.error(err.to_string()))?;
        Ok(client.database(&self.database))
    }

    fn block_on<T, F>(&self, future: F) -> Result<T, SendableError>
    where
        F: std::future::Future<Output = Result<T, SendableError>>,
    {
        self.runtime.clone().block_on(future)
    }
}

fn mongo_error(err: mongodb::error::Error) -> SendableError {
    STATEMENT_FAILED.error(err.to_string())
}

/// json → bson for filters, documents, and pipeline stages.
fn to_document(value: &Value, field: &str) -> Result<Document, SendableError> {
    match value {
        Value::Object(_) => mongodb::bson::to_document(value).map_err(|err| {
            INVALID_STATEMENT.error(format!("'{field}' is not a valid document: {err}"))
        }),
        Value::Null => Ok(Document::new()),
        _ => Err(INVALID_STATEMENT.error(format!("'{field}' must be an object"))),
    }
}

/// bson → json using relaxed extended json, which keeps object ids and dates readable while
/// staying valid json for downstream rexrap expressions.
fn to_json_object(document: Document) -> Map<String, Value> {
    match Bson::Document(document).into_relaxed_extjson() {
        Value::Object(map) => map,
        other => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
    }
}

fn document_parts(statement: &StatementSpec) -> Result<(&str, &DocumentCommand), SendableError> {
    match statement {
        StatementSpec::Document {
            collection,
            command,
            ..
        } => Ok((collection.as_str(), command)),
        StatementSpec::Sql { .. } => {
            Err(INVALID_STATEMENT.error("sql statements are not supported by the mongodb engine"))
        }
    }
}

async fn run_find(
    collection: &Collection<Document>,
    filter: &Value,
    options: &DocumentOptions,
) -> Result<Vec<Map<String, Value>>, SendableError> {
    let mut find = collection.find(to_document(filter, "find")?);
    if let Some(projection) = &options.projection {
        find = find.projection(to_document(projection, "options.projection")?);
    }
    if let Some(sort) = &options.sort {
        find = find.sort(to_document(sort, "options.sort")?);
    }
    if let Some(limit) = options.limit {
        find = find.limit(limit);
    }
    if let Some(skip) = options.skip {
        find = find.skip(skip);
    }

    let mut cursor = find.await.map_err(mongo_error)?;
    let mut documents = Vec::new();
    while cursor.advance().await.map_err(mongo_error)? {
        documents.push(to_json_object(
            cursor.deserialize_current().map_err(mongo_error)?,
        ));
    }
    Ok(documents)
}

async fn run_aggregate(
    collection: &Collection<Document>,
    pipeline: &[Value],
) -> Result<Vec<Map<String, Value>>, SendableError> {
    let stages = pipeline
        .iter()
        .map(|stage| to_document(stage, "aggregate"))
        .collect::<Result<Vec<_>, _>>()?;

    let mut cursor = collection.aggregate(stages).await.map_err(mongo_error)?;
    let mut documents = Vec::new();
    while cursor.advance().await.map_err(mongo_error)? {
        documents.push(to_json_object(
            cursor.deserialize_current().map_err(mongo_error)?,
        ));
    }
    Ok(documents)
}

async fn run_rows(
    database: &Database,
    collection_name: &str,
    command: &DocumentCommand,
) -> Result<RowSet, SendableError> {
    let documents = match command {
        DocumentCommand::Find { filter, options } => {
            run_find(&database.collection(collection_name), filter, options).await?
        }
        DocumentCommand::Aggregate { pipeline } => {
            run_aggregate(&database.collection(collection_name), pipeline).await?
        }
        DocumentCommand::Raw { command } => {
            let result = database
                .run_command(to_document(command, "command")?)
                .await
                .map_err(mongo_error)?;
            vec![to_json_object(result)]
        }
        other => {
            return Err(INVALID_STATEMENT.error(format!(
                "'{}' does not return documents; use db.execute instead",
                other.label()
            )));
        }
    };

    Ok(RowSet::from_objects(documents))
}

async fn run_write(
    database: &Database,
    collection_name: &str,
    command: &DocumentCommand,
) -> Result<ExecOutcome, SendableError> {
    let collection: Collection<Document> = database.collection(collection_name);

    match command {
        DocumentCommand::Insert { documents } => {
            let docs = documents
                .iter()
                .map(|document| to_document(document, "insert"))
                .collect::<Result<Vec<_>, _>>()?;
            if docs.is_empty() {
                return Ok(ExecOutcome::default());
            }
            let result = collection.insert_many(docs).await.map_err(mongo_error)?;
            let inserted = result.inserted_ids.len() as u64;
            let last = result
                .inserted_ids
                .into_iter()
                .max_by_key(|(index, _)| *index)
                .map(|(_, id)| id.into_relaxed_extjson());
            Ok(ExecOutcome {
                rows_affected: inserted,
                last_insert_id: last,
            })
        }
        DocumentCommand::Update {
            filter,
            update,
            options,
        } => {
            let filter = to_document(filter, "update.filter")?;
            let update = to_document(update, "update")?;
            let multi = options.multi.unwrap_or(true);
            let upsert = options.upsert.unwrap_or(false);

            let result = if multi {
                collection
                    .update_many(filter, update)
                    .upsert(upsert)
                    .await
                    .map_err(mongo_error)?
            } else {
                collection
                    .update_one(filter, update)
                    .upsert(upsert)
                    .await
                    .map_err(mongo_error)?
            };

            Ok(ExecOutcome {
                rows_affected: result.modified_count,
                last_insert_id: result.upserted_id.map(|id| id.into_relaxed_extjson()),
            })
        }
        DocumentCommand::Delete { filter, options } => {
            let filter = to_document(filter, "delete")?;
            let result = if options.multi.unwrap_or(true) {
                collection.delete_many(filter).await.map_err(mongo_error)?
            } else {
                collection.delete_one(filter).await.map_err(mongo_error)?
            };
            Ok(ExecOutcome {
                rows_affected: result.deleted_count,
                last_insert_id: None,
            })
        }
        DocumentCommand::Raw { command } => {
            let result = database
                .run_command(to_document(command, "command")?)
                .await
                .map_err(mongo_error)?;
            // `n` is what mongo reports for write commands; anything else counts as zero.
            let affected = result
                .get_i32("n")
                .map(|n| n as u64)
                .unwrap_or_else(|_| result.get_i64("n").map(|n| n as u64).unwrap_or_default());
            Ok(ExecOutcome {
                rows_affected: affected,
                last_insert_id: None,
            })
        }
        other => Err(INVALID_STATEMENT.error(format!(
            "'{}' returns documents; use db.query instead",
            other.label()
        ))),
    }
}

impl DatabaseConnector for MongoConnector {
    fn ensure_database(
        &self,
        spec: &ProvisionSpec,
        timeout: Duration,
    ) -> Result<bool, SendableError> {
        let collections = spec.collections.clone();
        self.block_on(with_timeout(
            async move {
                let database = self.database().await?;
                let existing = database
                    .list_collection_names()
                    .await
                    .map_err(mongo_error)?;
                let mut created = false;

                for spec in &collections {
                    if !existing.contains(&spec.name) {
                        database
                            .create_collection(&spec.name)
                            .await
                            .map_err(mongo_error)?;
                        created = true;
                    }

                    if spec.indexes.is_empty() {
                        continue;
                    }
                    let collection: Collection<Document> = database.collection(&spec.name);
                    for index in &spec.indexes {
                        let options = IndexOptions::builder()
                            .unique(Some(index.unique))
                            .name(index.name.clone())
                            .build();
                        let model = IndexModel::builder()
                            .keys(to_document(&index.keys, "indexes.keys")?)
                            .options(options)
                            .build();
                        collection.create_index(model).await.map_err(mongo_error)?;
                    }
                }

                Ok(created)
            },
            timeout,
        ))
    }

    fn query(&self, statement: &StatementSpec, timeout: Duration) -> Result<RowSet, SendableError> {
        let (collection, command) = document_parts(statement)?;
        self.block_on(with_timeout(
            async move {
                let database = self.database().await?;
                run_rows(&database, collection, command).await
            },
            timeout,
        ))
    }

    fn execute(
        &self,
        statement: &StatementSpec,
        timeout: Duration,
    ) -> Result<ExecOutcome, SendableError> {
        let (collection, command) = document_parts(statement)?;
        self.block_on(with_timeout(
            async move {
                let database = self.database().await?;
                run_write(&database, collection, command).await
            },
            timeout,
        ))
    }

    /// mongo has no multi-statement transaction without a replica set, so `transactional` is
    /// rejected rather than silently ignored.
    fn script(
        &self,
        statements: &[StatementSpec],
        transactional: bool,
        timeout: Duration,
    ) -> Result<Vec<StepOutcome>, SendableError> {
        if transactional {
            return Err(UNSUPPORTED_ENGINE.error(
                "mongodb scripts cannot run in a transaction here; set 'transaction' to false",
            ));
        }

        let mut resolved = Vec::with_capacity(statements.len());
        for statement in statements {
            resolved.push(document_parts(statement)?);
        }

        self.block_on(with_timeout(
            async move {
                let database = self.database().await?;
                let mut outcomes = Vec::with_capacity(resolved.len());
                for (collection, command) in resolved {
                    let outcome = if command.returns_documents() {
                        StepOutcome::Rows(run_rows(&database, collection, command).await?)
                    } else {
                        StepOutcome::Affected(run_write(&database, collection, command).await?)
                    };
                    outcomes.push(outcome);
                }
                Ok(outcomes)
            },
            timeout,
        ))
    }

    fn seed(&self, seeds: &[SeedSpec], timeout: Duration) -> Result<u64, SendableError> {
        let seeds = seeds.to_vec();
        self.block_on(with_timeout(
            async move {
                let database = self.database().await?;
                let mut inserted = 0u64;
                for seed in &seeds {
                    if seed.rows.is_empty() {
                        continue;
                    }
                    let documents = seed
                        .rows
                        .iter()
                        .map(|row| {
                            mongodb::bson::to_document(&Value::Object(row.clone())).map_err(|err| {
                                INVALID_STATEMENT
                                    .error(format!("seed row is not a valid document: {err}"))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    let collection: Collection<Document> = database.collection(&seed.table);
                    let result = collection
                        .insert_many(documents)
                        .await
                        .map_err(mongo_error)?;
                    inserted += result.inserted_ids.len() as u64;
                }
                Ok(inserted)
            },
            timeout,
        ))
    }

    /// collections stand in for tables; column summaries come from a sampled document, since a
    /// document store has no declared schema to read.
    fn inspect(&self, timeout: Duration) -> Result<Vec<TableInfo>, SendableError> {
        self.block_on(with_timeout(
            async move {
                let database = self.database().await?;
                let names = database
                    .list_collection_names()
                    .await
                    .map_err(mongo_error)?;

                let mut tables = Vec::with_capacity(names.len());
                for name in names {
                    let collection: Collection<Document> = database.collection(&name);
                    let sample = collection
                        .find_one(Document::new())
                        .await
                        .map_err(mongo_error)?;
                    let columns = sample
                        .map(|document| {
                            document
                                .into_iter()
                                .map(|(key, value)| ColumnSummary {
                                    name: key,
                                    native_type: format!("{:?}", value.element_type()),
                                    nullable: matches!(value, Bson::Null),
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    tables.push(TableInfo {
                        name,
                        schema: Some(self.database.clone()),
                        columns,
                    });
                }

                Ok(tables)
            },
            timeout,
        ))
    }
}
