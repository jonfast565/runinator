use std::{
    fs::{self, File},
    io::Read,
};

use chrono::{TimeZone, Utc};
use flate2::read::GzDecoder;
use runinator_database::archive::{ArchiveRow, ArchiveTable};
use runinator_models::json;
use serde_json::Value;
use uuid::Uuid;

use super::{ARCHIVE_FILE_EXTENSION, write_archive_jsonl_files};

#[test]
fn archive_writer_exports_gzipped_jsonl() {
    let root = std::env::temp_dir().join(format!("runinator-archive-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();

    let created_at = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
    let rows = vec![
        ArchiveRow {
            mark_id: Uuid::new_v4(),
            table: ArchiveTable::WorkflowRuns,
            primary_key: Uuid::new_v4(),
            created_at,
            row: json!({ "id": "run-1", "status": "succeeded" }),
        },
        ArchiveRow {
            mark_id: Uuid::new_v4(),
            table: ArchiveTable::WorkflowRuns,
            primary_key: Uuid::new_v4(),
            created_at,
            row: json!({ "id": "run-2", "status": "failed" }),
        },
    ];

    write_archive_jsonl_files(&root, &rows).unwrap();

    let day_dir = root.join("2026-06-21");
    let files = fs::read_dir(&day_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    assert!(files[0].to_string_lossy().ends_with(ARCHIVE_FILE_EXTENSION));

    let mut content = String::new();
    GzDecoder::new(File::open(&files[0]).unwrap())
        .read_to_string(&mut content)
        .unwrap();

    assert!(content.starts_with('{'));
    assert!(!content.trim_start().starts_with('['));

    let lines = content.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let value = serde_json::from_str::<Value>(line).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["source_table"], "workflow_runs");
        assert!(value["row"].is_object());
    }

    fs::remove_dir_all(&root).ok();
}
