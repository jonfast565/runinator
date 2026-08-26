//! Row mapper for the VM-native file metadata table.

use super::*;

row_mapper!(row_to_stored_file(row) -> StoredFile {
    StoredFile {
        descriptor: FileDescriptor {
            id: row.get("id"),
            name: row.get("name"),
            path: row.get("path"),
            mime_type: row.get("mime_type"),
            size_bytes: row.get("size_bytes"),
            sha256: row.get("sha256"),
        },
        scope: FileScope::parse(&row.get::<String, _>("scope")).unwrap_or(FileScope::Staged),
        org_id: row.get("org_id"),
        owner_id: row.get("owner_id"),
        workflow_run_id: row.get("workflow_run_id"),
        uri: row.get("uri"),
        revision: row.get("revision"),
        current: row.get("is_current"),
        archived: row.get("archived"),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});
