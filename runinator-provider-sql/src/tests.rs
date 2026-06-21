use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use crate::dump::DumpDataRequest;
use crate::format::DumpFormat;
use crate::helpers::{next_available_stem, normalize_timeout, sanitize_file_stem};

#[test]
fn dump_format_reports_wire_values_and_artifact_metadata() {
    assert_eq!(DumpFormat::Excel.file_extension(), "xlsx");
    assert_eq!(
        DumpFormat::Excel.mime_type(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(DumpFormat::Excel.as_str(), "excel");
    assert!(DumpFormat::Excel.requires_sheet_name());

    assert_eq!(DumpFormat::Csv.file_extension(), "csv");
    assert_eq!(DumpFormat::Csv.mime_type(), "text/csv");
    assert_eq!(DumpFormat::Csv.as_str(), "csv");
    assert!(!DumpFormat::Csv.requires_sheet_name());
}

#[test]
fn dump_data_request_defaults_to_excel_format() {
    let request: DumpDataRequest = serde_json::from_value(json!({
        "database": "postgres",
        "connection_string": "postgres://example",
        "dump_folder": "/tmp/runinator-sql-test",
        "queries": [{ "sql": "select 1" }]
    }))
    .unwrap();

    assert_eq!(request.format.as_str(), "excel");
    assert_eq!(request.queries.len(), 1);
    assert_eq!(request.queries[0].name, None);
}

#[test]
fn normalize_timeout_uses_default_for_non_positive_values() {
    assert_eq!(normalize_timeout(0), Duration::from_secs(30));
    assert_eq!(normalize_timeout(-5), Duration::from_secs(30));
    assert_eq!(normalize_timeout(9), Duration::from_secs(9));
}

#[test]
fn sanitize_file_stem_replaces_path_unsafe_characters_and_trims_edges() {
    assert_eq!(sanitize_file_stem(" ..daily:report?. "), "daily_report_");
    assert_eq!(sanitize_file_stem("''"), "");
    assert_eq!(sanitize_file_stem("line\nbreak"), "line_break");
}

#[test]
fn next_available_stem_keeps_duplicate_names_unique() {
    let mut counts = HashMap::new();

    assert_eq!(next_available_stem("report".into(), &mut counts), "report");
    assert_eq!(
        next_available_stem("report".into(), &mut counts),
        "report_01"
    );
    assert_eq!(next_available_stem(String::new(), &mut counts), "query_01");
    assert_eq!(next_available_stem(String::new(), &mut counts), "query_02");
}
