//! sql dialect helpers shared by every backend.
//!
//! queries are authored once in sqlite/mysql `?`-placeholder style and rendered per dialect.
//! only the genuinely divergent fragments (placeholder style, boolean literal, row locking,
//! insert-or-ignore form) live here; the method bodies live in `operations`.

/// sql dialects supported by the generic backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlDialect {
    Sqlite,
    Postgres,
    MySql,
}

/// render a `?`-placeholder template into the dialect's placeholder style.
///
/// sqlite and mysql use positional `?`; postgres uses ordinal `$1, $2, ...`. callers bind values
/// in the order the `?` appear, repeating a value when the same column is referenced twice (so the
/// bind order is identical across dialects even though postgres assigns it a fresh ordinal).
pub(crate) fn render(dialect: SqlDialect, sql: &str) -> String {
    match dialect {
        SqlDialect::Sqlite | SqlDialect::MySql => sql.to_string(),
        SqlDialect::Postgres => {
            let mut out = String::with_capacity(sql.len() + 16);
            let mut index = 0;
            for ch in sql.chars() {
                if ch == '?' {
                    index += 1;
                    out.push('$');
                    out.push_str(&index.to_string());
                } else {
                    out.push(ch);
                }
            }
            out
        }
    }
}

/// quote an identifier that is a reserved word in some dialect (e.g. `trigger`, `key`).
///
/// sqlite and postgres accept these bare as column names; mysql requires backticks.
pub(crate) fn ident(dialect: SqlDialect, name: &str) -> String {
    match dialect {
        SqlDialect::Sqlite | SqlDialect::Postgres => name.to_string(),
        SqlDialect::MySql => format!("`{name}`"),
    }
}

/// build the conflict-resolution tail of an upsert in `?`-placeholder style, without `RETURNING`.
///
/// `conflict` names the unique columns; `set` is the comma-separated `col = ...` assignments using
/// the inserted values. postgres/sqlite reference the rejected row via `excluded`; mysql via
/// `VALUES()`. callers that need the upserted row back must read it separately on mysql, which has
/// no `RETURNING` for `ON DUPLICATE KEY UPDATE`.
pub(crate) fn on_conflict_update(dialect: SqlDialect, conflict: &str, columns: &[&str]) -> String {
    match dialect {
        SqlDialect::Sqlite | SqlDialect::Postgres => {
            let set = columns
                .iter()
                .map(|col| format!("{col} = excluded.{col}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ON CONFLICT({conflict}) DO UPDATE SET {set}")
        }
        SqlDialect::MySql => {
            let set = columns
                .iter()
                .map(|col| format!("{col} = VALUES({col})"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ON DUPLICATE KEY UPDATE {set}")
        }
    }
}

/// boolean true literal for an `enabled = ...` comparison.
pub(crate) fn bool_true(dialect: SqlDialect) -> &'static str {
    match dialect {
        // sqlite and mysql store booleans as integers.
        SqlDialect::Sqlite | SqlDialect::MySql => "1",
        SqlDialect::Postgres => "TRUE",
    }
}

/// row-locking suffix for a claim subselect, empty where the dialect cannot skip locked rows.
pub(crate) fn skip_locked(dialect: SqlDialect) -> &'static str {
    match dialect {
        // sqlite serializes writers, so there is nothing to skip.
        SqlDialect::Sqlite => "",
        SqlDialect::Postgres | SqlDialect::MySql => " FOR UPDATE SKIP LOCKED",
    }
}

/// build an insert that ignores unique-constraint conflicts, in `?`-placeholder style.
///
/// `conflict` names the conflicting columns (used by postgres `ON CONFLICT`). `returning`, when set,
/// appends a `RETURNING` clause; mysql cannot return rows and must read them back separately.
pub(crate) fn insert_ignore(
    dialect: SqlDialect,
    table: &str,
    columns: &str,
    values: &str,
    conflict: &str,
    returning: Option<&str>,
) -> String {
    let mut sql = match dialect {
        SqlDialect::Sqlite => {
            format!("INSERT OR IGNORE INTO {table} ({columns}) VALUES ({values})")
        }
        SqlDialect::MySql => {
            format!("INSERT IGNORE INTO {table} ({columns}) VALUES ({values})")
        }
        SqlDialect::Postgres => format!(
            "INSERT INTO {table} ({columns}) VALUES ({values}) ON CONFLICT({conflict}) DO NOTHING"
        ),
    };
    if let Some(returning) = returning {
        sql.push_str(" RETURNING ");
        sql.push_str(returning);
    }
    sql
}
