#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct ExportedFile {
    pub name: String,
    pub rows: usize,
    pub path: PathBuf,
    pub mime_type: String,
    pub size_bytes: i64,
    pub format: ExportFormat,
}

impl ExportedFile {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "rows": self.rows,
            "path": self.path,
            "format": self.format.as_str(),
            "size_bytes": self.size_bytes,
        })
    }

    pub fn to_artifact(&self) -> NewRunArtifact {
        NewRunArtifact {
            name: self
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.name.clone()),
            mime_type: self.mime_type.clone(),
            size_bytes: self.size_bytes,
            uri: self.path.to_string_lossy().into_owned(),
            metadata: json!({
                "provider": "db",
                "statement_name": self.name,
                "rows": self.rows,
                "format": self.format.as_str(),
            })
            .into(),
        }
    }
}
