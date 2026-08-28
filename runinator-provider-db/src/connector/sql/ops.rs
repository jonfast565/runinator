use runinator_models::errors::SendableError;
use serde_json::Value;

use crate::errors::STATEMENT_FAILED;

/// a single statement to run inside a script, already resolved to text, parameters, and whether
/// its result should be collected as rows or as an affected count.
pub struct SqlStep {
    pub text: String,
    pub params: Vec<Value>,
    pub returns_rows: bool,
}

/// guess whether a statement produces rows from its leading keyword, so a script step does not
/// have to be annotated in the common case. `insert … returning` is the notable exception that
/// the keyword alone gets wrong, so the whole text is checked for `returning` too.
pub fn sql_returns_rows(text: &str) -> bool {
    let trimmed = text.trim_start().trim_start_matches('(').trim_start();
    let leading = trimmed
        .split(|ch: char| ch.is_whitespace() || ch == '(')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    let row_returning = matches!(
        leading.as_str(),
        "select"
            | "with"
            | "show"
            | "pragma"
            | "explain"
            | "describe"
            | "desc"
            | "values"
            | "table"
    );
    if row_returning {
        return true;
    }

    // `returning` promotes a write into a row-returning statement on postgres and sqlite.
    text.to_ascii_lowercase()
        .split_whitespace()
        .any(|word| word.trim_matches(|ch: char| !ch.is_alphanumeric()) == "returning")
}

pub fn statement_error(err: sqlx::Error) -> SendableError {
    STATEMENT_FAILED.error(err.to_string())
}

/// generate the per-engine statement runners. every backend needs the same control flow but
/// sqlx's types are distinct per database, so the bodies are emitted rather than made generic.
macro_rules! sql_ops {
    (
        $module:ident,
        pool = $pool:ty,
        db = $db:ty,
        bind = $bind:path,
        columns = $columns:path,
        value = $value:path,
        last_insert_id = $last_insert_id:expr
    ) => {
        pub mod $module {
            use std::time::Duration;

            use runinator_models::errors::SendableError;
            use serde_json::Value;
            use sqlx::{Column, Executor, TypeInfo};

            use super::{SqlStep, statement_error};
            use crate::connector::timeout::with_timeout;
            use crate::rowset::{ColumnInfo, ExecOutcome, RowSet, StepOutcome};

            type Pool = $pool;

            fn build_query<'q>(
                text: &'q str,
                params: &'q [Value],
            ) -> sqlx::query::Query<'q, $db, <$db as sqlx::Database>::Arguments<'q>> {
                let mut query = sqlx::query(text);
                for param in params {
                    query = $bind(query, param);
                }
                query
            }

            /// column metadata for a result set that came back empty, so exports still get headers.
            async fn describe_columns(pool: &Pool, text: &str) -> Vec<ColumnInfo> {
                let Ok(described) = pool.describe(text).await else {
                    return Vec::new();
                };
                described
                    .columns()
                    .iter()
                    .map(|column| {
                        let native = column.type_info().name().to_string();
                        ColumnInfo::new(column.name(), super::super::decode::kind_for(&native))
                            .with_native_type(native)
                    })
                    .collect()
            }

            async fn fetch_rows(
                pool: &Pool,
                text: &str,
                params: &[Value],
            ) -> Result<RowSet, SendableError> {
                let rows = build_query(text, params)
                    .fetch_all(pool)
                    .await
                    .map_err(statement_error)?;

                let Some(first) = rows.first() else {
                    return Ok(RowSet::new(describe_columns(pool, text).await, Vec::new()));
                };

                let columns = $columns(first);
                let data = rows
                    .iter()
                    .map(|row| {
                        columns
                            .iter()
                            .enumerate()
                            .map(|(idx, column)| {
                                let native = column.native_type.as_deref().unwrap_or_default();
                                $value(row, idx, native)
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();

                Ok(RowSet::new(columns, data))
            }

            async fn run_statement(
                pool: &Pool,
                text: &str,
                params: &[Value],
            ) -> Result<ExecOutcome, SendableError> {
                let result = build_query(text, params)
                    .execute(pool)
                    .await
                    .map_err(statement_error)?;
                Ok(ExecOutcome {
                    rows_affected: result.rows_affected(),
                    last_insert_id: ($last_insert_id)(&result),
                })
            }

            pub async fn query(
                pool: &Pool,
                text: &str,
                params: &[Value],
                timeout: Duration,
            ) -> Result<RowSet, SendableError> {
                with_timeout(fetch_rows(pool, text, params), timeout).await
            }

            pub async fn execute(
                pool: &Pool,
                text: &str,
                params: &[Value],
                timeout: Duration,
            ) -> Result<ExecOutcome, SendableError> {
                with_timeout(run_statement(pool, text, params), timeout).await
            }

            /// run every step, optionally as one transaction. on failure inside a transaction the
            /// guard is dropped without a commit, so sqlx rolls the whole script back.
            pub async fn script(
                pool: &Pool,
                steps: &[SqlStep],
                transactional: bool,
                timeout: Duration,
            ) -> Result<Vec<StepOutcome>, SendableError> {
                if !transactional {
                    let mut outcomes = Vec::with_capacity(steps.len());
                    for step in steps {
                        outcomes.push(run_step(pool, step, timeout).await?);
                    }
                    return Ok(outcomes);
                }

                let mut transaction = pool.begin().await.map_err(statement_error)?;
                let mut outcomes = Vec::with_capacity(steps.len());
                for step in steps {
                    let outcome = if step.returns_rows {
                        let rows = build_query(&step.text, &step.params)
                            .fetch_all(&mut *transaction)
                            .await
                            .map_err(statement_error)?;
                        let columns = rows.first().map($columns).unwrap_or_default();
                        let data = rows
                            .iter()
                            .map(|row| {
                                columns
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, column)| {
                                        let native =
                                            column.native_type.as_deref().unwrap_or_default();
                                        $value(row, idx, native)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>();
                        StepOutcome::Rows(RowSet::new(columns, data))
                    } else {
                        let result = build_query(&step.text, &step.params)
                            .execute(&mut *transaction)
                            .await
                            .map_err(statement_error)?;
                        StepOutcome::Affected(ExecOutcome {
                            rows_affected: result.rows_affected(),
                            last_insert_id: ($last_insert_id)(&result),
                        })
                    };
                    outcomes.push(outcome);
                }

                transaction.commit().await.map_err(statement_error)?;
                let _ = timeout;
                Ok(outcomes)
            }

            async fn run_step(
                pool: &Pool,
                step: &SqlStep,
                timeout: Duration,
            ) -> Result<StepOutcome, SendableError> {
                if step.returns_rows {
                    return Ok(StepOutcome::Rows(
                        query(pool, &step.text, &step.params, timeout).await?,
                    ));
                }
                Ok(StepOutcome::Affected(
                    execute(pool, &step.text, &step.params, timeout).await?,
                ))
            }
        }
    };
}

#[cfg(feature = "postgres")]
fn bind_pg<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    param: &'q Value,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match param {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(flag) => query.bind(*flag),
        Value::Number(number) if number.is_i64() => query.bind(number.as_i64()),
        Value::Number(number) if number.is_u64() => {
            query.bind(number.as_u64().map(|value| value as i64))
        }
        Value::Number(number) => query.bind(number.as_f64()),
        Value::String(text) => query.bind(text.as_str()),
        // arrays and objects go over as jsonb, which is the only faithful postgres mapping.
        other => query.bind(other.clone()),
    }
}

#[cfg(feature = "mariadb")]
fn bind_mysql<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    param: &'q Value,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match param {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(flag) => query.bind(*flag),
        Value::Number(number) if number.is_i64() => query.bind(number.as_i64()),
        Value::Number(number) if number.is_u64() => query.bind(number.as_u64()),
        Value::Number(number) => query.bind(number.as_f64()),
        Value::String(text) => query.bind(text.as_str()),
        other => query.bind(other.to_string()),
    }
}

#[cfg(feature = "sqlite")]
fn bind_sqlite<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    param: &'q Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match param {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(flag) => query.bind(*flag),
        Value::Number(number) if number.is_i64() => query.bind(number.as_i64()),
        Value::Number(number) if number.is_u64() => {
            query.bind(number.as_u64().map(|value| value as i64))
        }
        Value::Number(number) => query.bind(number.as_f64()),
        Value::String(text) => query.bind(text.as_str()),
        other => query.bind(other.to_string()),
    }
}

#[cfg(feature = "postgres")]
fn pg_last_insert_id(_result: &sqlx::postgres::PgQueryResult) -> Option<Value> {
    // postgres has no implicit last-insert id; callers use `insert … returning id` instead.
    None
}

#[cfg(feature = "mariadb")]
fn mysql_last_insert_id(result: &sqlx::mysql::MySqlQueryResult) -> Option<Value> {
    let id = result.last_insert_id();
    (id != 0).then(|| Value::Number(id.into()))
}

#[cfg(feature = "sqlite")]
fn sqlite_last_insert_id(result: &sqlx::sqlite::SqliteQueryResult) -> Option<Value> {
    let id = result.last_insert_rowid();
    (id != 0).then(|| Value::Number(id.into()))
}

#[cfg(feature = "postgres")]
sql_ops!(
    pg,
    pool = sqlx::PgPool,
    db = sqlx::Postgres,
    bind = super::bind_pg,
    columns = crate::connector::sql::decode::columns_pg,
    value = crate::connector::sql::decode::value_pg,
    last_insert_id = super::pg_last_insert_id
);

#[cfg(feature = "mariadb")]
sql_ops!(
    mysql,
    pool = sqlx::MySqlPool,
    db = sqlx::MySql,
    bind = super::bind_mysql,
    columns = crate::connector::sql::decode::columns_mysql,
    value = crate::connector::sql::decode::value_mysql,
    last_insert_id = super::mysql_last_insert_id
);

#[cfg(feature = "sqlite")]
sql_ops!(
    sqlite,
    pool = sqlx::SqlitePool,
    db = sqlx::Sqlite,
    bind = super::bind_sqlite,
    columns = crate::connector::sql::decode::columns_sqlite,
    value = crate::connector::sql::decode::value_sqlite,
    last_insert_id = super::sqlite_last_insert_id
);
