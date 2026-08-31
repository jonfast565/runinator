//! schema-shape lints over the migrated database, rather than over any one operation.
//!
//! deleting a parent row makes the engine look for surviving children of every foreign key that
//! points at it. with no index on the child column that search is a full scan of the child table —
//! once per deleted parent row — so the cost stays invisible until the child table grows, and then
//! appears as an operation that never finishes. `delete_workflow` hit exactly that: it deletes a
//! workflow's node runs, and `workflow_orchestration_events.workflow_node_run_id` had no index.
//!
//! this reads sqlite because that is the schema every workspace has; postgres is its hand-maintained
//! sibling, kept in step by `migration_parity_tests`. mysql needs no entry of its own — innodb
//! creates an index for every foreign key column on its own.

use super::*;
use sqlx::Row;
use std::collections::BTreeSet;

/// foreign key columns deliberately left without an index, each with the reason it is safe.
///
/// empty on purpose: an unindexed foreign key is a scan waiting for its table to grow, and every
/// column in this schema is on a table that grows with runtime history. an entry here needs to argue
/// that its child table is bounded, not merely that it is small today.
const UNINDEXED_ALLOWED: &[(&str, &str, &str)] = &[];

#[tokio::test]
async fn consolidated_schema_keeps_the_bcnf_table_budget() {
    let path = std::env::temp_dir().join(format!(
        "runinator-schema-budget-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let schema_tables = tables(&db).await;
    assert_eq!(
        schema_tables.len(),
        79,
        "the greenfield schema table budget changed"
    );
    for removed in [
        "runs",
        "run_chunks",
        "run_artifacts",
        "org_memberships",
        "team_members",
        "org_quotas",
        "workflow_run_execution_states",
    ] {
        assert!(
            !schema_tables.iter().any(|table| table == removed),
            "legacy table {removed} was reintroduced"
        );
    }

    let admission_columns = table_columns(&db, "ingress_admissions").await;
    assert!(admission_columns.contains("workflow_id"));
    assert!(admission_columns.contains("pipeline_id"));
    assert!(!admission_columns.contains("target_kind"));
    assert!(!admission_columns.contains("target_id"));

    let binding_columns = table_columns(&db, "orchestration_bindings").await;
    for derived in ["org_id", "scope", "correlation_key", "pipeline_id"] {
        assert!(
            !binding_columns.contains(derived),
            "derived binding column {derived} was reintroduced"
        );
    }
    assert!(
        !table_columns(&db, "orchestration_correlation_aliases")
            .await
            .contains("generation")
    );

    let indexes = sqlx::query("PRAGMA index_list(\"pipeline_runs\")")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert!(indexes.iter().any(|row| {
        row.get::<String, _>("name") == "idx_pipeline_runs_orchestration"
            && row.get::<i64, _>("unique") == 1
    }));

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn every_foreign_key_column_leads_an_index() {
    let path = std::env::temp_dir().join(format!(
        "runinator-schema-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let mut missing = Vec::new();
    for table in tables(&db).await {
        let indexed = leading_index_columns(&db, &table).await;
        for column in foreign_key_columns(&db, &table).await {
            let allowed = UNINDEXED_ALLOWED
                .iter()
                .any(|(allowed_table, allowed_column, _)| {
                    *allowed_table == table && *allowed_column == column
                });

            if indexed.contains(&column) || allowed {
                continue;
            }

            missing.push(format!("{table}.{column}"));
        }
    }

    let _ = fs::remove_file(path);

    assert!(
        missing.is_empty(),
        "foreign key columns no index leads on: {missing:?}. each one turns a delete of the parent \
         row into a full scan of the child table. add the index to a migration in all three dialect \
         directories, or list the column in UNINDEXED_ALLOWED with the reason its table stays bounded"
    );
}

#[tokio::test]
async fn unindexed_allowances_still_name_real_foreign_keys() {
    // an allowance whose column was renamed or indexed would silently widen what the lint tolerates.
    if UNINDEXED_ALLOWED.is_empty() {
        return;
    }

    let path = std::env::temp_dir().join(format!(
        "runinator-schema-allow-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    for (table, column, reason) in UNINDEXED_ALLOWED {
        let is_foreign_key = foreign_key_columns(&db, table).await.contains(*column);
        let is_indexed = leading_index_columns(&db, table).await.contains(*column);
        assert!(
            is_foreign_key && !is_indexed,
            "UNINDEXED_ALLOWED lists {table}.{column} ({reason}), but it is no longer an unindexed \
             foreign key column; drop the entry"
        );
    }

    let _ = fs::remove_file(path);
}

async fn tables(db: &SqliteDb) -> Vec<String> {
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         AND name <> '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    rows.iter()
        .map(|row| row.get::<String, _>("name"))
        .collect()
}

async fn table_columns(db: &SqliteDb, table: &str) -> BTreeSet<String> {
    sqlx::query(&format!("PRAGMA table_info(\"{table}\")"))
        .fetch_all(db.pool())
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect()
}

/// the leading column of every foreign key on `table`.
///
/// a composite key only needs an index leading on its first column, which is the `seq = 0` row.
async fn foreign_key_columns(db: &SqliteDb, table: &str) -> BTreeSet<String> {
    let rows = sqlx::query(&format!("PRAGMA foreign_key_list(\"{table}\")"))
        .fetch_all(db.pool())
        .await
        .unwrap();
    rows.iter()
        .filter(|row| row.get::<i64, _>("seq") == 0)
        .map(|row| row.get::<String, _>("from"))
        .collect()
}

/// every column some index on `table` leads on, which is what a foreign key check can seek into.
async fn leading_index_columns(db: &SqliteDb, table: &str) -> BTreeSet<String> {
    let indexes = sqlx::query(&format!("PRAGMA index_list(\"{table}\")"))
        .fetch_all(db.pool())
        .await
        .unwrap();
    let mut leading = BTreeSet::new();

    for index in &indexes {
        // a partial index covers only the rows matching its predicate, so it cannot answer whether
        // any child still references the parent.
        if index.get::<i64, _>("partial") != 0 {
            continue;
        }

        let name = index.get::<String, _>("name");
        let columns = sqlx::query(&format!("PRAGMA index_info(\"{name}\")"))
            .fetch_all(db.pool())
            .await
            .unwrap();
        // an expression index reports a null column name; it leads on no column we can match.
        if let Some(first) = columns
            .iter()
            .find(|column| column.get::<i64, _>("seqno") == 0)
            && let Ok(column) = first.try_get::<String, _>("name")
        {
            leading.insert(column);
        }
    }

    leading
}
