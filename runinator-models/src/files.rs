//! Immutable workflow-input file descriptors.
//!
//! The descriptor is the only file shape that may cross a workflow/worker boundary. Storage
//! locations stay server-side so an action never gains authority to read arbitrary blob keys.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDescriptor {
    pub id: Uuid,
    pub name: String,
    /// Relative path within the operator-selected folder, or the filename for a lone file.
    pub path: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256: String,
}

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

/// Durable metadata for a file object. The opaque storage URI remains server-side and is excluded
/// from the command-center descriptor wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFile {
    pub descriptor: FileDescriptor,
    pub scope: FileScope,
    pub org_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    pub workflow_run_id: Option<Uuid>,
    pub uri: String,
    pub revision: i64,
    pub current: bool,
    pub archived: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl FileDescriptor {
    pub fn to_value(&self) -> Value {
        let mut value = Value::encode(self).unwrap_or(Value::Null);
        if let Value::Object(map) = &mut value {
            map.remove("local_path");
        }
        value
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        let descriptor = value.decode::<Self>().map_err(|_| {
            "file descriptor requires id, name, path, mime_type, size_bytes, and sha256".to_string()
        })?;
        validate_relative_path(&descriptor.path)?;
        if descriptor.name.trim().is_empty() || descriptor.mime_type.trim().is_empty() {
            return Err("file descriptor name and mime_type are required".into());
        }
        if descriptor.size_bytes < 0 {
            return Err("file descriptor size_bytes cannot be negative".into());
        }
        if descriptor.sha256.len() != 64
            || !descriptor
                .sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err("file descriptor sha256 must be a 64-character hex digest".into());
        }
        Ok(descriptor)
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
