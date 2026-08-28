use std::collections::HashMap;

use runinator_plugin::cancel::CancellationToken;
use serde_json::{Value, json};

use crate::actions::Shape;
#[cfg(any(feature = "postgres", feature = "mariadb", feature = "sqlite"))]
use crate::connector::sql::ops::sql_returns_rows;
use crate::engine::Engine;
use crate::export::{ExportFormat, ExportSpec, export_rows};
use crate::helpers::{next_available_stem, normalize_timeout, sanitize_file_stem};
use crate::rowset::{ColumnInfo, ColumnKind, ExecOutcome, RowSet};
use crate::statement::{StatementFields, StatementInput, StatementSpec};

fn fields(value: Value) -> StatementFields {
    serde_json::from_value(value).expect("statement fields should deserialize")
}

fn sample_rows() -> RowSet {
    RowSet::new(
        vec![
            ColumnInfo::new("id", ColumnKind::Integer),
            ColumnInfo::new("email", ColumnKind::String),
            ColumnInfo::new("active", ColumnKind::Boolean),
        ],
        vec![
            vec![json!(1), json!("a@example.com"), json!(true)],
            vec![json!(2), Value::Null, json!(false)],
        ],
    )
}

#[test]
fn engine_wire_values_round_trip() {
    for (wire, engine) in [
        ("sqlite", Engine::Sqlite),
        ("postgres", Engine::Postgres),
        ("mariadb", Engine::Mariadb),
    ] {
        let parsed: Engine = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(parsed, engine);
        assert_eq!(engine.as_str(), wire);
    }

    assert!(serde_json::from_value::<Engine>(json!("mysql")).is_err());
    assert!(serde_json::from_value::<Engine>(json!("mongodb")).is_err());
    assert_eq!(Engine::Postgres.placeholder(2), "$3");
    assert_eq!(Engine::Mariadb.placeholder(2), "?");
    assert_eq!(Engine::Sqlite.placeholder(0), "?");
}

#[test]
fn sql_engines_require_sql() {
    let resolved = StatementSpec::resolve(
        fields(json!({"sql": "select 1", "params": [7]})),
        Engine::Sqlite,
    )
    .unwrap();
    match resolved {
        StatementSpec::Sql { text, params, .. } => {
            assert_eq!(text, "select 1");
            assert_eq!(params, vec![json!(7)]);
        }
    }

    let missing = StatementSpec::resolve(fields(json!({})), Engine::Postgres).unwrap_err();
    assert!(missing.to_string().contains("DB003"), "{missing}");

    let empty =
        StatementSpec::resolve(fields(json!({"sql": "   "})), Engine::Postgres).unwrap_err();
    assert!(empty.to_string().contains("must not be empty"), "{empty}");
}

#[test]
fn a_bare_string_is_a_valid_statement_entry() {
    let inputs: Vec<StatementInput> =
        serde_json::from_value(json!(["select 1", {"sql": "select 2", "name": "second"}])).unwrap();

    let mut inputs = inputs.into_iter();
    let first = inputs.next().unwrap().into_fields();
    assert_eq!(first.sql.as_deref(), Some("select 1"));
    assert!(first.name.is_none());

    let second = inputs.next().unwrap().into_fields();
    assert_eq!(second.sql.as_deref(), Some("select 2"));
    assert_eq!(second.name.as_deref(), Some("second"));
}

#[test]
fn rows_shape_keeps_json_types_and_table_shape_stringifies() {
    let rows = sample_rows();

    let Value::Object(typed) = Shape::Rows.project(&rows) else {
        panic!("rows shape should be an object");
    };
    assert_eq!(typed["row_count"], json!(2));
    assert_eq!(
        typed["rows"],
        json!([
            {"id": 1, "email": "a@example.com", "active": true},
            {"id": 2, "email": null, "active": false},
        ])
    );
    // the column types travel with the rows so downstream nodes can branch on them.
    assert_eq!(typed["columns"][0]["type"], json!("integer"));
    assert_eq!(typed["columns"][2]["type"], json!("boolean"));

    let Value::Object(flat) = Shape::Table.project(&rows) else {
        panic!("table shape should be an object");
    };
    assert_eq!(flat["headers"], json!(["id", "email", "active"]));
    // every cell is a string here, and null renders as empty for spreadsheet readers.
    assert_eq!(
        flat["rows"],
        json!([["1", "a@example.com", "true"], ["2", "", "false"]])
    );
}

#[test]
fn exec_outcome_reports_a_null_last_insert_id_when_absent() {
    let outcome = ExecOutcome {
        rows_affected: 3,
        last_insert_id: None,
    };
    assert_eq!(
        outcome.to_json(),
        json!({"rows_affected": 3, "last_insert_id": null})
    );
}

#[cfg(any(feature = "postgres", feature = "mariadb", feature = "sqlite"))]
#[test]
fn row_returning_statements_are_detected_from_their_leading_keyword() {
    for text in [
        "select 1",
        "  SELECT * from users",
        "with cte as (select 1) select * from cte",
        "pragma table_info(users)",
        "SHOW TABLES",
        "(select 1)",
    ] {
        assert!(sql_returns_rows(text), "expected rows for {text:?}");
    }

    for text in [
        "insert into users (id) values (1)",
        "update users set active = true",
        "delete from users",
        "create table users (id integer)",
    ] {
        assert!(!sql_returns_rows(text), "expected no rows for {text:?}");
    }

    // `returning` promotes a write into a row-returning statement.
    assert!(sql_returns_rows(
        "insert into users (id) values (1) returning id"
    ));
    assert!(sql_returns_rows(
        "delete from users where id = 1 RETURNING *"
    ));
}

#[test]
fn helpers_normalize_timeouts_and_file_stems() {
    assert_eq!(normalize_timeout(0).as_secs(), 30);
    assert_eq!(normalize_timeout(-5).as_secs(), 30);
    assert_eq!(normalize_timeout(90).as_secs(), 90);

    assert_eq!(sanitize_file_stem("re/port:name"), "re_port_name");
    // trimming runs dots-then-quotes, so a dot outside a quote survives.
    assert_eq!(sanitize_file_stem("  '.trimmed.'  "), ".trimmed.");
    assert_eq!(sanitize_file_stem("  .trimmed.  "), "trimmed");
    assert_eq!(sanitize_file_stem(""), "");

    let mut counts = HashMap::new();
    assert_eq!(next_available_stem("report".into(), &mut counts), "report");
    assert_eq!(
        next_available_stem("report".into(), &mut counts),
        "report_01"
    );
    assert_eq!(
        next_available_stem(String::new(), &mut counts),
        "statement_01"
    );
}

#[test]
fn exporting_a_row_set_writes_a_file_and_describes_it_as_an_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let spec = ExportSpec {
        folder: directory.path().to_string_lossy().into_owned(),
        format: ExportFormat::Csv,
        name: Some("Active Users".to_string()),
        file_prefix: Some("dump_".to_string()),
    };

    let rows = sample_rows();
    let mut counts = HashMap::new();
    let exported = export_rows(&rows, &spec, "fallback", 0, &mut counts).unwrap();

    assert!(exported.path.exists());
    assert_eq!(exported.rows, 2);
    assert_eq!(exported.format, ExportFormat::Csv);
    assert_eq!(exported.mime_type, "text/csv");
    assert!(exported.size_bytes > 0);
    assert_eq!(
        exported.path.file_name().unwrap().to_string_lossy(),
        "dump_Active Users.csv"
    );

    let contents = std::fs::read_to_string(&exported.path).unwrap();
    assert!(contents.contains("id,email,active"), "{contents}");
    assert!(contents.contains("a@example.com"), "{contents}");

    let artifact = exported.to_artifact();
    assert_eq!(artifact.mime_type, "text/csv");
    assert_eq!(artifact.size_bytes, exported.size_bytes);
    let metadata: Value = artifact.metadata.into();
    assert_eq!(metadata["provider"], json!("db"));
    assert_eq!(metadata["rows"], json!(2));

    // a second export of the same name must not overwrite the first.
    let again = export_rows(&rows, &spec, "fallback", 1, &mut counts).unwrap();
    assert_ne!(again.path, exported.path);
}

#[test]
fn export_format_wire_values_match_their_files() {
    assert_eq!(ExportFormat::Excel.file_extension(), "xlsx");
    assert!(ExportFormat::Excel.requires_sheet_name());
    assert_eq!(ExportFormat::Excel.as_str(), "excel");
    assert_eq!(ExportFormat::Csv.file_extension(), "csv");
    assert!(!ExportFormat::Csv.requires_sheet_name());
    assert_eq!(ExportFormat::default(), ExportFormat::Excel);
}

/// the full provision → execute → query lifecycle against a real sqlite file. sqlite is bundled
/// with sqlx, so this runs with no external service.
#[test]
fn sqlite_round_trips_provision_execute_and_query() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested").join("app.db");
    let connection = format!("sqlite://{}", path.display());
    let token = CancellationToken::new();

    let provisioned = crate::actions::provision::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "schema": [
                "create table users (id integer primary key, email text, active boolean)"
            ],
            "seed": [{
                "table": "users",
                "rows": [
                    {"id": 1, "email": "a@example.com", "active": true},
                    {"id": 2, "email": null, "active": false}
                ]
            }]
        }),
        30,
        token.clone(),
    )
    .unwrap();

    let output: Value = provisioned.output_json.unwrap().into();
    assert_eq!(output["created"], json!(true), "{output}");
    assert_eq!(output["applied"], json!(1));
    assert_eq!(output["seeded"], json!(2));
    // provisioning creates the parent directory, not just the file.
    assert!(path.exists());

    let executed = crate::actions::execute::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "sql": "update users set active = ? where id = ?",
            "params": [true, 2]
        }),
        30,
        token.clone(),
    )
    .unwrap();
    let output: Value = executed.output_json.unwrap().into();
    assert_eq!(output["rows_affected"], json!(1), "{output}");

    let queried = crate::actions::query::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "sql": "select id, email, active from users order by id",
        }),
        30,
        token.clone(),
    )
    .unwrap();
    let output: Value = queried.output_json.unwrap().into();
    assert_eq!(output["row_count"], json!(2), "{output}");
    // the point of the default shape: real integers, not "1".
    assert_eq!(output["rows"][0]["id"], json!(1));
    assert_eq!(output["rows"][1]["email"], Value::Null);
    assert_eq!(output["shape"], json!("rows"));

    let inspected = crate::actions::inspect::run(
        json!({ "engine": "sqlite", "connection": connection }),
        30,
        token,
    )
    .unwrap();
    let output: Value = inspected.output_json.unwrap().into();
    assert_eq!(output["table_count"], json!(1), "{output}");
    assert_eq!(output["tables"][0]["name"], json!("users"));
    assert_eq!(output["tables"][0]["columns"][0]["name"], json!("id"));
}

#[test]
fn a_failing_script_step_rolls_the_transaction_back() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tx.db");
    let connection = format!("sqlite://{}", path.display());
    let token = CancellationToken::new();

    crate::actions::provision::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "schema": ["create table t (id integer primary key)"]
        }),
        30,
        token.clone(),
    )
    .unwrap();

    let failed = crate::actions::script::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "transaction": true,
            "statements": [
                "insert into t (id) values (1)",
                "insert into t (id) values (1)"
            ]
        }),
        30,
        token.clone(),
    )
    .unwrap_err();
    assert!(failed.to_string().contains("DB007"), "{failed}");

    let queried = crate::actions::query::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "sql": "select count(*) as total from t"
        }),
        30,
        token,
    )
    .unwrap();
    let output: Value = queried.output_json.unwrap().into();
    // the duplicate key aborted the whole script, so the first insert is gone too.
    assert_eq!(output["rows"][0]["total"], json!(0), "{output}");
}

#[test]
fn a_script_reports_row_steps_and_affected_steps_separately() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("script.db");
    let connection = format!("sqlite://{}", path.display());
    let token = CancellationToken::new();

    crate::actions::provision::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "schema": ["create table t (id integer primary key, label text)"]
        }),
        30,
        token.clone(),
    )
    .unwrap();

    let result = crate::actions::script::run(
        json!({
            "engine": "sqlite",
            "connection": connection,
            "statements": [
                {"name": "seed", "sql": "insert into t (id, label) values (?, ?)", "params": [1, "one"]},
                {"name": "read_back", "sql": "select id, label from t"}
            ]
        }),
        30,
        token,
    )
    .unwrap();

    let output: Value = result.output_json.unwrap().into();
    assert_eq!(output["step_count"], json!(2), "{output}");
    assert_eq!(output["steps"][0]["kind"], json!("affected"));
    assert_eq!(output["steps"][0]["name"], json!("seed"));
    assert_eq!(output["steps"][0]["rows_affected"], json!(1));
    assert_eq!(output["steps"][1]["kind"], json!("rows"));
    assert_eq!(output["steps"][1]["rows"][0]["label"], json!("one"));
    assert_eq!(output["row_count"], json!(1));
    assert_eq!(output["rows_affected"], json!(1));
}

#[test]
fn provisioning_an_existing_database_reports_it_as_already_present() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.db");
    let connection = format!("sqlite://{}", path.display());
    let token = CancellationToken::new();

    let request = json!({
        "engine": "sqlite",
        "connection": connection,
        "schema": ["create table if not exists t (id integer primary key)"]
    });

    let first = crate::actions::provision::run(request.clone(), 30, token.clone()).unwrap();
    let first: Value = first.output_json.unwrap().into();
    assert_eq!(first["created"], json!(true));

    // re-running is idempotent, which is what makes provision safe to leave in a workflow.
    let second = crate::actions::provision::run(request, 30, token).unwrap();
    let second: Value = second.output_json.unwrap().into();
    assert_eq!(second["created"], json!(false));
}

#[test]
fn a_canceled_token_stops_the_action_before_it_connects() {
    let token = CancellationToken::new();
    token.cancel();

    let error = crate::actions::query::run(
        json!({
            "engine": "sqlite",
            "connection": "sqlite:///nonexistent/should-not-be-reached.db",
            "sql": "select 1"
        }),
        30,
        token,
    )
    .unwrap_err();
    assert!(error.to_string().contains("DB009"), "{error}");
}

#[test]
fn querying_a_missing_sqlite_file_fails_instead_of_creating_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("absent.db");
    let connection = format!("sqlite://{}", path.display());

    let error = crate::actions::query::run(
        json!({ "engine": "sqlite", "connection": connection, "sql": "select 1" }),
        30,
        CancellationToken::new(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("DB004"), "{error}");
    assert!(!path.exists(), "a query must not create the database");
}

#[test]
fn retired_document_fields_are_rejected() {
    let error = crate::actions::query::run(
        json!({
            "engine": "sqlite",
            "connection": "sqlite::memory:",
            "sql": "select 1",
            "collection": "retired"
        }),
        30,
        CancellationToken::new(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("collection"), "{error}");
}

#[test]
fn an_unsupported_action_names_the_call_it_rejected() {
    use runinator_models::runs::ProviderExecutionRequest;
    use runinator_plugin::provider::Provider;

    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "db".to_string(),
        action_function: "truncate_everything".to_string(),
        parameters: json!({}).into(),
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
    };

    let error = crate::DbProvider
        .execute_service(request, None, CancellationToken::new())
        .unwrap_err();
    assert!(error.to_string().contains("DB001"), "{error}");
    assert!(error.to_string().contains("truncate_everything"), "{error}");
}

#[test]
fn provider_metadata_is_valid_and_covers_every_action() {
    use runinator_models::providers::validate_provider_metadata;
    use runinator_plugin::provider::Provider;

    let metadata = crate::DbProvider.metadata();
    assert_eq!(metadata.name, "db");
    validate_provider_metadata(&metadata).expect("provider metadata should validate");

    let functions = metadata
        .actions
        .iter()
        .map(|action| action.function_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        functions,
        ["query", "execute", "script", "provision", "inspect"]
    );

    // the connection string must stay secret so authors reference it as `secret.*`.
    for action in &metadata.actions {
        let connection = action
            .parameters
            .iter()
            .find(|parameter| parameter.name == "connection")
            .expect("every action takes a connection");
        assert!(
            connection.secret,
            "{} leaks its connection",
            action.function_name
        );
        assert!(connection.required);
    }
}

/// DECIMAL/NUMERIC is the one column type with no lossless json representation: a json number is
/// an f64, and both engines allow precisions well past what that holds. these pin the boundary
/// where the decoder stops emitting a number and starts preserving digits as text.
#[cfg(any(feature = "postgres", feature = "mariadb"))]
mod decimals {
    use crate::connector::sql::decode::decimal_to_json;
    use bigdecimal::BigDecimal;
    use serde_json::{Value, json};
    use std::str::FromStr;

    fn rendered(literal: &str) -> Value {
        decimal_to_json(&BigDecimal::from_str(literal).expect("literal should parse"))
    }

    #[test]
    fn ordinary_precision_stays_a_json_number() {
        assert_eq!(rendered("9.99"), json!(9.99));
        assert_eq!(rendered("2.50"), json!(2.5));
        assert_eq!(rendered("-123.456"), json!(-123.456));
        assert_eq!(rendered("0"), json!(0.0));
        assert_eq!(rendered("1000000"), json!(1000000.0));
    }

    #[test]
    fn precision_wider_than_f64_keeps_every_digit_as_text() {
        // 30 significant digits: an f64 carries ~15-17, so emitting a number here would drop
        // digits silently. the exact value survives as a string instead.
        let wide = "123456789012345678901234567890";
        assert_eq!(rendered(wide), json!(wide));

        let wide_fraction = "0.123456789012345678901234567890";
        assert_eq!(rendered(wide_fraction), json!(wide_fraction));
    }

    #[test]
    fn the_string_fallback_preserves_the_columns_declared_scale() {
        // decimal(40,10) holding a value too wide for f64 keeps its trailing zeros, so callers can
        // still see the scale the column was declared with.
        assert_eq!(
            rendered("12345678901234567890.1234500000"),
            json!("12345678901234567890.1234500000")
        );
    }

    #[test]
    fn a_number_is_only_emitted_when_it_round_trips_exactly() {
        // whatever the decoder emits as a json number must parse back to the identical decimal.
        for literal in [
            "9.99",
            "2.50",
            "0.1",
            "-0.0001",
            "123456789012345678901234567890",
            "0.123456789012345678901234567890",
        ] {
            let original = BigDecimal::from_str(literal).expect("literal should parse");
            if let Value::Number(number) = decimal_to_json(&original) {
                let round_tripped =
                    BigDecimal::from_str(&number.to_string()).expect("emitted number should parse");
                assert_eq!(
                    round_tripped, original,
                    "{literal} was emitted as a lossy number"
                );
            }
        }
    }
}

#[test]
fn the_error_dictionary_is_numbered_in_order() {
    use runinator_models::errors::ProviderErrors;

    let dictionary = crate::DbProvider::error_dictionary();
    assert_eq!(dictionary.len(), 11);
    for (index, descriptor) in dictionary.iter().enumerate() {
        assert_eq!(descriptor.code, format!("DB{:03}", index + 1));
    }
}
