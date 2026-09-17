#[allow(unused_imports)]
use super::*;

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
        if !runinator_hash::is_valid_hex(&descriptor.sha256) {
            return Err("file descriptor sha256 must be a 64-character hex digest".into());
        }
        Ok(descriptor)
    }
}
