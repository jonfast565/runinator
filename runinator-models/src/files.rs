//! Immutable workflow-input file descriptors.
//!
//! The descriptor is the only file shape that may cross a workflow/worker boundary. Storage
//! locations stay server-side so an action never gains authority to read arbitrary blob keys.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{Map, Value};

/// Lifecycle location of the descriptor. A staged file is visible only to its owner until the run
/// that claims it begins; a library file is the latest revision at a virtual path; a run file is
/// an immutable snapshot referenced by that run's VM parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileScope {
    Staged,
    Library,
    Run,
}

impl FileScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Library => "library",
            Self::Run => "run",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "staged" => Some(Self::Staged),
            "library" => Some(Self::Library),
            "run" => Some(Self::Run),
            _ => None,
        }
    }
}

/// Reject host paths and traversal before a user-provided folder name reaches an object key or a
/// worker workspace. Backslashes are normalized by the UI before upload; accepting them here
/// would make behavior differ by host platform.
pub fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("file path must be a safe, non-empty relative path".into());
    }
    Ok(())
}

/// Add the transient local materialization path used by worker-side providers. It is intentionally
/// not part of [`FileDescriptor`], so persisted run parameters stay portable.
pub fn with_local_path(descriptor: &FileDescriptor, local_path: String) -> Value {
    let mut object = match descriptor.to_value() {
        Value::Object(object) => object,
        _ => Map::new(),
    };
    object.insert("local_path".into(), Value::String(local_path));
    Value::Object(object)
}

/// IDs of descriptors embedded anywhere in a portable workflow parameter value. The run-start
/// boundary uses this to ensure every descriptor is claimed with the run rather than trusting a
/// caller-maintained side list.
pub fn referenced_file_ids(value: &Value) -> Vec<Uuid> {
    fn visit(value: &Value, ids: &mut BTreeSet<Uuid>) {
        if let Ok(descriptor) = FileDescriptor::from_value(value) {
            ids.insert(descriptor.id);
            return;
        }
        match value {
            Value::Array(values) => values.iter().for_each(|value| visit(value, ids)),
            Value::Object(values) => values.values().for_each(|value| visit(value, ids)),
            _ => {}
        }
    }

    let mut ids = BTreeSet::new();
    visit(value, &mut ids);
    ids.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_escape_a_folder() {
        assert!(validate_relative_path("assets/logo.png").is_ok());
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("/tmp/file").is_err());
    }

    #[test]
    fn finds_file_descriptors_in_nested_values() {
        let descriptor = FileDescriptor {
            id: Uuid::nil(),
            name: "report.csv".into(),
            path: "reports/report.csv".into(),
            mime_type: "text/csv".into(),
            size_bytes: 2,
            sha256: "a".repeat(64),
        };
        let value = crate::json!({
            "attachments": [descriptor.to_value()],
            "metadata": { "keep": true },
        });

        assert_eq!(referenced_file_ids(&value), vec![Uuid::nil()]);
    }
}

mod file_descriptor;
pub use file_descriptor::FileDescriptor;

mod stored_file;
pub use stored_file::StoredFile;
