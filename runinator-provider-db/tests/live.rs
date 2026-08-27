//! live-engine coverage for the postgres, mysql/mariadb, and mongodb connectors. these exercise
//! the real drivers through the public provider entry point, so they cover the decode, bind, and
//! provisioning paths that the in-crate sqlite tests cannot reach.
//!
//! bring the engines up with:
//!
//! ```sh
//! docker compose -f runinator-provider-db/tests/docker-compose.yml up -d
//! ```
//!
//! then run them with the matching URL env vars set, e.g.
//!
//! ```sh
//! RUNINATOR_TEST_POSTGRES_URL=postgres://runi:runi@127.0.0.1:55432/runi \
//!   cargo test -p runinator-provider-db --test live -- --ignored
//! ```

use runinator_models::runs::ProviderExecutionRequest;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::Provider;
use runinator_provider_db::DbProvider;
use serde_json::{Value, json};

/// call an action through the provider trait, the same way the worker does.
fn call(function: &str, parameters: Value) -> Result<Value, String> {
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "db".to_string(),
        action_function: function.to_string(),
        parameters: parameters.into(),
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
    };

    DbProvider
        .execute_service(request, None, CancellationToken::new())
        .map(|result| result.output_json.map(Into::into).unwrap_or(Value::Null))
        .map_err(|err| err.to_string())
}

fn ok(function: &str, parameters: Value) -> Value {
    match call(function, parameters) {
        Ok(output) => output,
        Err(err) => panic!("db.{function} failed: {err}"),
    }
}

fn url(variable: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            eprintln!("skipping live database test; set {variable}");
            None
        }
    }
}

// ---------------------------------------------------------------- postgres

#[test]
#[ignore = "requires a reachable PostgreSQL server; set RUNINATOR_TEST_POSTGRES_URL"]
fn postgres_round_trips_the_whole_action_surface() {
    let Some(connection) = url("RUNINATOR_TEST_POSTGRES_URL") else {
        return;
    };

    ok(
        "provision",
        json!({
            "engine": "postgres",
            "connection": connection,
            "schema": [
                "drop table if exists live_users",
                "create table live_users (id integer primary key, email text, active boolean, score double precision)"
            ],
            "seed": [{
                "table": "live_users",
                "rows": [
                    {"id": 1, "email": "a@example.com", "active": true, "score": 1.5},
                    {"id": 2, "email": null, "active": false, "score": 2.0}
                ]
            }]
        }),
    );

    // bound parameters use $n on postgres, and a bound bool must stay a bool.
    let queried = ok(
        "query",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "select id, email, active, score from live_users where active = $1 order by id",
            "params": [true]
        }),
    );
    assert_eq!(queried["row_count"], json!(1), "{queried}");
    assert_eq!(queried["rows"][0]["id"], json!(1));
    assert_eq!(queried["rows"][0]["active"], json!(true));
    assert_eq!(queried["rows"][0]["score"], json!(1.5));
    assert_eq!(queried["columns"][0]["type"], json!("integer"));
    assert_eq!(queried["columns"][2]["type"], json!("boolean"));

    // a null text column must survive as json null, not as an empty string.
    let all = ok(
        "query",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "select id, email from live_users order by id"
        }),
    );
    assert_eq!(all["rows"][1]["email"], Value::Null, "{all}");

    let executed = ok(
        "execute",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "update live_users set active = $1 where id = $2",
            "params": [true, 2]
        }),
    );
    assert_eq!(executed["rows_affected"], json!(1), "{executed}");
    // postgres has no implicit last-insert id.
    assert_eq!(executed["last_insert_id"], Value::Null);

    let table = ok(
        "query",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "select id, email from live_users order by id",
            "shape": "table"
        }),
    );
    assert_eq!(table["headers"], json!(["id", "email"]));
    assert_eq!(table["rows"], json!([["1", "a@example.com"], ["2", ""]]));

    let inspected = ok(
        "inspect",
        json!({ "engine": "postgres", "connection": connection }),
    );
    let tables = inspected["tables"].as_array().expect("tables array");
    let users = tables
        .iter()
        .find(|table| table["name"] == json!("live_users"))
        .expect("live_users should be listed");
    assert_eq!(users["schema"], json!("public"));
    assert_eq!(users["columns"][0]["name"], json!("id"));
    assert_eq!(users["columns"][1]["nullable"], json!(true));

    ok(
        "execute",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "drop table live_users"
        }),
    );
}

#[test]
#[ignore = "requires a reachable PostgreSQL server; set RUNINATOR_TEST_POSTGRES_URL"]
fn postgres_decodes_its_native_types_into_json() {
    let Some(connection) = url("RUNINATOR_TEST_POSTGRES_URL") else {
        return;
    };

    ok(
        "script",
        json!({
            "engine": "postgres",
            "connection": connection,
            "statements": [
                "drop table if exists live_types",
                "create table live_types (\
                   small smallint, medium integer, big bigint, ratio real, amount double precision, \
                   exact numeric(10,2), wide numeric(40,8), \
                   flag boolean, name text, payload jsonb, uid uuid, \
                   moment timestamptz, day date, raw bytea, nothing text)",
                "insert into live_types values (\
                   1, 2, 3, 1.5, 2.5, 9.99, 12345678901234567890.12345678, \
                   true, 'hello', '{\"k\":[1,2]}'::jsonb, \
                   '00000000-0000-0000-0000-000000000001'::uuid, \
                   '2026-07-26T12:00:00Z'::timestamptz, '2026-07-26'::date, '\\x0102'::bytea, null)"
            ]
        }),
    );

    let queried = ok(
        "query",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "select * from live_types"
        }),
    );
    let row = &queried["rows"][0];

    assert_eq!(row["small"], json!(1), "{queried}");
    assert_eq!(row["medium"], json!(2));
    assert_eq!(row["big"], json!(3));
    assert_eq!(row["ratio"], json!(1.5));
    assert_eq!(row["amount"], json!(2.5));
    assert_eq!(row["flag"], json!(true));
    assert_eq!(row["name"], json!("hello"));
    // jsonb comes back as real nested json, not as a re-encoded string.
    assert_eq!(row["payload"], json!({"k": [1, 2]}));
    assert_eq!(row["uid"], json!("00000000-0000-0000-0000-000000000001"));
    assert_eq!(row["day"], json!("2026-07-26"));
    assert!(
        row["moment"]
            .as_str()
            .unwrap_or_default()
            .starts_with("2026-07-26T12:00:00"),
        "unexpected timestamptz {}",
        row["moment"]
    );
    assert_eq!(row["raw"], json!("\\x0102"));
    assert_eq!(row["nothing"], Value::Null);
    // numeric decodes exactly, and renders as a json number while the value still fits an f64.
    assert_eq!(row["exact"], json!(9.99), "{queried}");
    // past that it keeps every digit as text rather than rounding without saying so.
    assert_eq!(
        row["wide"],
        json!("12345678901234567890.12345678"),
        "{queried}"
    );

    ok(
        "execute",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "drop table live_types"
        }),
    );
}

#[test]
#[ignore = "requires a reachable PostgreSQL server; set RUNINATOR_TEST_POSTGRES_URL"]
fn postgres_rolls_a_failed_transaction_back_and_detects_returning() {
    let Some(connection) = url("RUNINATOR_TEST_POSTGRES_URL") else {
        return;
    };

    ok(
        "script",
        json!({
            "engine": "postgres",
            "connection": connection,
            "statements": [
                "drop table if exists live_tx",
                "create table live_tx (id integer primary key)"
            ]
        }),
    );

    // `insert … returning` is a write that yields rows; the step must be reported as rows.
    let returning = ok(
        "script",
        json!({
            "engine": "postgres",
            "connection": connection,
            "statements": [
                {"name": "insert_one", "sql": "insert into live_tx (id) values (1) returning id"}
            ]
        }),
    );
    assert_eq!(returning["steps"][0]["kind"], json!("rows"), "{returning}");
    assert_eq!(returning["steps"][0]["rows"][0]["id"], json!(1));

    let failed = call(
        "script",
        json!({
            "engine": "postgres",
            "connection": connection,
            "transaction": true,
            "statements": [
                "insert into live_tx (id) values (2)",
                "insert into live_tx (id) values (1)"
            ]
        }),
    )
    .expect_err("a duplicate key must fail the script");
    assert!(failed.contains("DB007"), "{failed}");

    let count = ok(
        "query",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "select count(*)::bigint as total from live_tx"
        }),
    );
    // the successful first insert was rolled back with the failing one.
    assert_eq!(count["rows"][0]["total"], json!(1), "{count}");

    ok(
        "execute",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": "drop table live_tx"
        }),
    );
}

#[test]
#[ignore = "requires a reachable PostgreSQL server; set RUNINATOR_TEST_POSTGRES_URL"]
fn postgres_creates_a_missing_database_through_the_admin_connection() {
    let Some(connection) = url("RUNINATOR_TEST_POSTGRES_URL") else {
        return;
    };

    let target = format!("runi_live_{}", std::process::id());
    let created_url = swap_database(&connection, &target);

    let provisioned = ok(
        "provision",
        json!({
            "engine": "postgres",
            "connection": created_url,
            "admin_connection": connection,
            "schema": ["create table t (id integer primary key)"]
        }),
    );
    assert_eq!(provisioned["created"], json!(true), "{provisioned}");
    assert_eq!(provisioned["applied"], json!(1));

    // re-provisioning an existing database is a no-op, which is what makes it safe in a workflow.
    let again = ok(
        "provision",
        json!({
            "engine": "postgres",
            "connection": created_url,
            "admin_connection": connection,
            "schema": ["create table if not exists t (id integer primary key)"]
        }),
    );
    assert_eq!(again["created"], json!(false), "{again}");

    ok(
        "execute",
        json!({
            "engine": "postgres",
            "connection": connection,
            "sql": format!("drop database \"{target}\" with (force)")
        }),
    );
}

/// replace the database segment of a connection URL, keeping credentials and host intact.
fn swap_database(connection: &str, database: &str) -> String {
    let (prefix, _) = connection.rsplit_once('/').expect("url should have a path");
    format!("{prefix}/{database}")
}

// ------------------------------------------------------------- mysql/maria

#[test]
#[ignore = "requires a reachable MySQL or MariaDB server; set RUNINATOR_TEST_MYSQL_URL"]
fn mysql_round_trips_the_whole_action_surface() {
    let Some(connection) = url("RUNINATOR_TEST_MYSQL_URL") else {
        return;
    };

    ok(
        "provision",
        json!({
            "engine": "mysql",
            "connection": connection,
            "schema": [
                "drop table if exists live_users",
                "create table live_users (id int primary key auto_increment, email varchar(128), active boolean, score double)"
            ],
            "seed": [{
                "table": "live_users",
                "rows": [
                    {"id": 1, "email": "a@example.com", "active": true, "score": 1.5},
                    {"id": 2, "email": null, "active": false, "score": 2.0}
                ]
            }]
        }),
    );

    // mysql uses ? placeholders rather than $n.
    let queried = ok(
        "query",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "select id, email, active, score from live_users where id = ? order by id",
            "params": [1]
        }),
    );
    assert_eq!(queried["row_count"], json!(1), "{queried}");
    assert_eq!(queried["rows"][0]["id"], json!(1));
    assert_eq!(queried["rows"][0]["score"], json!(1.5));
    // mysql has no distinct boolean type — `boolean` is `tinyint(1)` — but both engines report the
    // column as BOOLEAN, and a value of 0/1 is exactly what a real boolean holds, so it narrows.
    assert_eq!(queried["rows"][0]["active"], json!(true));

    let nulls = ok(
        "query",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "select id, email from live_users order by id"
        }),
    );
    assert_eq!(nulls["rows"][1]["email"], Value::Null, "{nulls}");

    // an auto-increment insert reports the generated id.
    let inserted = ok(
        "execute",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "insert into live_users (email, active, score) values (?, ?, ?)",
            "params": ["c@example.com", true, 3.5]
        }),
    );
    assert_eq!(inserted["rows_affected"], json!(1), "{inserted}");
    assert!(
        inserted["last_insert_id"].as_i64().unwrap_or_default() > 2,
        "expected a generated id, got {}",
        inserted["last_insert_id"]
    );

    let table = ok(
        "query",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "select id, email from live_users where id <= 2 order by id",
            "shape": "table"
        }),
    );
    assert_eq!(table["headers"], json!(["id", "email"]));
    assert_eq!(table["rows"], json!([["1", "a@example.com"], ["2", ""]]));

    let inspected = ok(
        "inspect",
        json!({ "engine": "mysql", "connection": connection }),
    );
    let tables = inspected["tables"].as_array().expect("tables array");
    let users = tables
        .iter()
        .find(|table| table["name"] == json!("live_users"))
        .expect("live_users should be listed");
    assert_eq!(users["columns"][0]["name"], json!("id"));

    ok(
        "execute",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "drop table live_users"
        }),
    );
}

#[test]
#[ignore = "requires a reachable MySQL or MariaDB server; set RUNINATOR_TEST_MYSQL_URL"]
fn mysql_decodes_its_native_types_into_json() {
    let Some(connection) = url("RUNINATOR_TEST_MYSQL_URL") else {
        return;
    };

    ok(
        "script",
        json!({
            "engine": "mysql",
            "connection": connection,
            "statements": [
                "drop table if exists live_types",
                "create table live_types (\
                   tiny tinyint, small smallint, medium int, big bigint, ubig bigint unsigned, \
                   ratio float, amount double, exact decimal(10,2), wide decimal(40,8), \
                   name varchar(32), \
                   payload json, moment datetime, day date, raw blob, nothing varchar(8))",
                "insert into live_types values (\
                   1, 2, 3, 4, 5, 1.5, 2.5, 9.99, 12345678901234567890.12345678, 'hello', '{\"k\":[1,2]}', \
                   '2026-07-26 12:00:00', '2026-07-26', x'0102', null)"
            ]
        }),
    );

    let queried = ok(
        "query",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "select * from live_types"
        }),
    );
    let row = &queried["rows"][0];

    assert_eq!(row["tiny"], json!(1), "{queried}");
    assert_eq!(row["small"], json!(2));
    assert_eq!(row["medium"], json!(3));
    assert_eq!(row["big"], json!(4));
    assert_eq!(row["ubig"], json!(5));
    assert_eq!(row["ratio"], json!(1.5));
    assert_eq!(row["amount"], json!(2.5));
    assert_eq!(row["name"], json!("hello"));
    assert_eq!(row["payload"], json!({"k": [1, 2]}));
    assert_eq!(row["day"], json!("2026-07-26"));
    assert!(
        row["moment"]
            .as_str()
            .unwrap_or_default()
            .starts_with("2026-07-26T12:00:00"),
        "unexpected datetime {}",
        row["moment"]
    );
    assert_eq!(row["raw"], json!("\\x0102"));
    assert_eq!(row["nothing"], Value::Null);
    assert_eq!(row["exact"], json!(9.99), "{queried}");
    assert_eq!(
        row["wide"],
        json!("12345678901234567890.12345678"),
        "{queried}"
    );

    ok(
        "execute",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "drop table live_types"
        }),
    );
}

/// the cases where the mysql protocol, or mariadb specifically, reports two different things under
/// one type name. this must pass identically against mysql 8 and mariadb 11 — that is the point.
#[test]
#[ignore = "requires a reachable MySQL or MariaDB server; set RUNINATOR_TEST_MYSQL_URL"]
fn mysql_resolves_the_type_names_both_engines_overload() {
    let Some(connection) = url("RUNINATOR_TEST_MYSQL_URL") else {
        return;
    };

    ok(
        "script",
        json!({
            "engine": "mysql",
            "connection": connection,
            "statements": [
                "drop table if exists live_ambiguous",
                "create table live_ambiguous (\
                   flag boolean, counter tinyint(1), doc json, scalar_doc json, \
                   blob_bin blob, blob_digits blob, width bit(8), made year)",
                "insert into live_ambiguous values (\
                   true, 4, '{\"k\":[1,2]}', '123', x'0304', '123', b'10101010', 2026)"
            ]
        }),
    );

    let queried = ok(
        "query",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "select * from live_ambiguous"
        }),
    );
    let row = &queried["rows"][0];

    // `boolean` and `tinyint(1)` are the same column type and both report BOOLEAN. 0/1 is what a
    // real boolean holds, so it narrows; anything else keeps its value instead of becoming `true`.
    assert_eq!(row["flag"], json!(true), "{queried}");
    assert_eq!(row["counter"], json!(4), "{queried}");

    // mariadb reports a `json` column as BLOB, mysql 8 as JSON. both land on the document.
    assert_eq!(row["doc"], json!({"k": [1, 2]}), "{queried}");

    // a blob is still a blob, even when its bytes happen to parse as json.
    assert_eq!(row["blob_bin"], json!("\\x0304"), "{queried}");
    assert_eq!(row["blob_digits"], json!("\\x313233"), "{queried}");

    assert_eq!(row["width"], json!(170), "{queried}");
    assert_eq!(row["made"], json!(2026), "{queried}");

    // column metadata has to agree with the values above, not with the raw type name.
    let kind = |name: &str| {
        queried["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .find(|column| column["name"] == json!(name))
            .unwrap_or_else(|| panic!("no column {name} in {queried}"))["type"]
            .clone()
    };
    assert_eq!(kind("flag"), json!("boolean"), "{queried}");
    assert_eq!(kind("counter"), json!("integer"), "{queried}");
    assert_eq!(kind("doc"), json!("json"), "{queried}");
    assert_eq!(kind("blob_bin"), json!("binary"), "{queried}");
    assert_eq!(kind("width"), json!("integer"), "{queried}");
    assert_eq!(kind("made"), json!("integer"), "{queried}");

    ok(
        "execute",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "drop table live_ambiguous"
        }),
    );
}

#[test]
#[ignore = "requires a reachable MySQL or MariaDB server; set RUNINATOR_TEST_MYSQL_URL"]
fn mysql_rolls_a_failed_transaction_back() {
    let Some(connection) = url("RUNINATOR_TEST_MYSQL_URL") else {
        return;
    };

    ok(
        "script",
        json!({
            "engine": "mysql",
            "connection": connection,
            "statements": [
                "drop table if exists live_tx",
                // innodb is required for a rollback to mean anything.
                "create table live_tx (id int primary key) engine=innodb"
            ]
        }),
    );

    let failed = call(
        "script",
        json!({
            "engine": "mysql",
            "connection": connection,
            "transaction": true,
            "statements": [
                "insert into live_tx (id) values (1)",
                "insert into live_tx (id) values (1)"
            ]
        }),
    )
    .expect_err("a duplicate key must fail the script");
    assert!(failed.contains("DB007"), "{failed}");

    let count = ok(
        "query",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "select count(*) as total from live_tx"
        }),
    );
    assert_eq!(count["rows"][0]["total"], json!(0), "{count}");

    ok(
        "execute",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": "drop table live_tx"
        }),
    );
}

#[test]
#[ignore = "requires a reachable MySQL or MariaDB server; set RUNINATOR_TEST_MYSQL_URL"]
fn mysql_creates_a_missing_database_through_the_admin_connection() {
    let Some(connection) = url("RUNINATOR_TEST_MYSQL_URL") else {
        return;
    };

    let target = format!("runi_live_{}", std::process::id());
    let created_url = swap_database(&connection, &target);

    let provisioned = ok(
        "provision",
        json!({
            "engine": "mysql",
            "connection": created_url,
            "admin_connection": connection,
            "schema": ["create table t (id int primary key)"]
        }),
    );
    assert_eq!(provisioned["created"], json!(true), "{provisioned}");

    let again = ok(
        "provision",
        json!({
            "engine": "mysql",
            "connection": created_url,
            "admin_connection": connection,
            "schema": ["create table if not exists t (id int primary key)"]
        }),
    );
    assert_eq!(again["created"], json!(false), "{again}");

    ok(
        "execute",
        json!({
            "engine": "mysql",
            "connection": connection,
            "sql": format!("drop database `{target}`")
        }),
    );
}

// ----------------------------------------------------------------- mongodb

#[cfg(feature = "mongo")]
mod mongo {
    use super::{call, json, ok, url};

    #[test]
    #[ignore = "requires a reachable MongoDB server; set RUNINATOR_TEST_MONGO_URL"]
    fn mongo_round_trips_documents_counts_and_collections() {
        let Some(connection) = url("RUNINATOR_TEST_MONGO_URL") else {
            return;
        };

        // start from a clean collection so re-runs are deterministic.
        ok(
            "execute",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "delete": {}
            }),
        );

        let provisioned = ok(
            "provision",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collections": [{
                    "name": "live_users",
                    "indexes": [{"keys": {"email": 1}, "name": "email_idx", "unique": true}]
                }],
                "seed": [{
                    "collection": "live_users",
                    "rows": [
                        {"id": 1, "email": "a@example.com", "active": true, "score": 1.5},
                        {"id": 2, "email": "b@example.com", "active": false, "score": 2.5}
                    ]
                }]
            }),
        );
        assert_eq!(provisioned["seeded"], json!(2), "{provisioned}");

        // a find returns typed documents, with keys unioned across the result set.
        let found = ok(
            "query",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "find": {"active": true},
                "options": {"projection": {"_id": 0, "id": 1, "email": 1, "score": 1}}
            }),
        );
        assert_eq!(found["row_count"], json!(1), "{found}");
        assert_eq!(found["rows"][0]["id"], json!(1));
        assert_eq!(found["rows"][0]["email"], json!("a@example.com"));
        assert_eq!(found["rows"][0]["score"], json!(1.5));

        let sorted = ok(
            "query",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "find": {},
                "options": {"projection": {"_id": 0, "id": 1}, "sort": {"id": -1}, "limit": 1}
            }),
        );
        assert_eq!(sorted["rows"][0]["id"], json!(2), "{sorted}");

        let aggregated = ok(
            "query",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "aggregate": [
                    {"$group": {"_id": null, "total": {"$sum": "$score"}}},
                    {"$project": {"_id": 0, "total": 1}}
                ]
            }),
        );
        assert_eq!(aggregated["rows"][0]["total"], json!(4.0), "{aggregated}");

        let updated = ok(
            "execute",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "update": {"filter": {"id": 2}, "set": {"active": true}}
            }),
        );
        assert_eq!(updated["rows_affected"], json!(1), "{updated}");

        let inserted = ok(
            "execute",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "insert": [{"id": 3, "email": "c@example.com", "active": false}]
            }),
        );
        assert_eq!(inserted["rows_affected"], json!(1), "{inserted}");
        assert!(!inserted["last_insert_id"].is_null());

        let deleted = ok(
            "execute",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "delete": {"id": 3}
            }),
        );
        assert_eq!(deleted["rows_affected"], json!(1), "{deleted}");

        // a raw command is the document-store equivalent of running a bare statement.
        let raw = ok(
            "query",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "command": {"ping": 1}
            }),
        );
        assert_eq!(raw["rows"][0]["ok"], json!(1.0), "{raw}");

        let inspected = ok(
            "inspect",
            json!({ "engine": "mongodb", "connection": connection }),
        );
        let tables = inspected["tables"].as_array().expect("tables array");
        assert!(
            tables
                .iter()
                .any(|table| table["name"] == json!("live_users")),
            "{inspected}"
        );

        // the unique index from provisioning is real: a duplicate email must be rejected.
        let duplicate = call(
            "execute",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "insert": [{"id": 9, "email": "a@example.com"}]
            }),
        )
        .expect_err("the unique index should reject a duplicate email");
        assert!(duplicate.contains("DB007"), "{duplicate}");

        ok(
            "execute",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "collection": "live_users",
                "delete": {}
            }),
        );
    }

    #[test]
    #[ignore = "requires a reachable MongoDB server; set RUNINATOR_TEST_MONGO_URL"]
    fn mongo_rejects_sql_and_transactional_scripts() {
        let Some(connection) = url("RUNINATOR_TEST_MONGO_URL") else {
            return;
        };

        let sql = call(
            "query",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "sql": "select 1"
            }),
        )
        .expect_err("sql must be rejected on a document store");
        assert!(sql.contains("DB003"), "{sql}");

        let transactional = call(
            "script",
            json!({
                "engine": "mongodb",
                "connection": connection,
                "transaction": true,
                "statements": [{"collection": "live_users", "find": {}}]
            }),
        )
        .expect_err("a transactional mongo script must be refused, not silently run");
        assert!(transactional.contains("DB011"), "{transactional}");

        // A Mongo URL without a database cannot be scoped, so report that clearly.
        let no_database = call(
            "query",
            json!({
                "engine": "mongodb",
                "connection": "mongodb://127.0.0.1:57017",
                "collection": "t",
                "find": {}
            }),
        )
        .expect_err("a url with no database must be rejected");
        assert!(no_database.contains("DB005"), "{no_database}");
    }
}
