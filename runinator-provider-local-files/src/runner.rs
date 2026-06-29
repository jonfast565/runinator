use std::path::Path;

use runinator_models::{
    errors::SendableError,
    json,
    runs::{NewRunArtifact, ProviderExecutionRequest, TaskExecutionResult},
};

use crate::errors::{IO, NOT_A_DIRECTORY, NOT_A_FILE, UNKNOWN_ACTION, WRITE_DISABLED};
use crate::params::{PathParams, WriteParams, parse_params};
use crate::sandbox::{self, LOCATION_LOCAL};

// only inline file contents into the result when small and valid utf-8; larger or binary files are
// represented purely by the captured artifact.
const INLINE_CONTENT_LIMIT: u64 = 1_048_576;

/// dispatch a local-files action by its function name.
pub(crate) fn execute(
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let root = sandbox::root()?;
    match request.action_function.as_str() {
        "read_file" => read_file(request, &root),
        "write_file" => write_file(request, &root),
        "list_dir" => list_dir(request, &root),
        "stat" => stat(request, &root),
        "delete" => delete(request, &root),
        other => Err(UNKNOWN_ACTION.error(other)),
    }
}

fn read_file(
    request: &ProviderExecutionRequest,
    root: &Path,
) -> Result<TaskExecutionResult, SendableError> {
    let params: PathParams = parse_params(request)?;
    let target = sandbox::resolve_existing(root, &params.path)?;
    if !target.is_file() {
        return Err(NOT_A_FILE.error(&params.path));
    }

    let bytes =
        std::fs::read(&target).map_err(|err| IO.error(format!("{}: {err}", params.path)))?;
    let size_bytes = bytes.len() as i64;
    let mime_type = mime_guess::from_path(&target)
        .first_or_octet_stream()
        .to_string();

    // capture a copy in the run's artifact dir so the file is preserved as a run artifact. the uri
    // is a path on this machine, so the artifact is tagged `local`: it is only retrievable while
    // this desktop worker is connected, unlike a cloud-stored artifact the web service can stream.
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let dest = Path::new(&request.artifact_dir).join(&file_name);
    std::fs::write(&dest, &bytes).map_err(|err| IO.error(format!("{}: {err}", dest.display())))?;

    let artifact = NewRunArtifact {
        name: file_name,
        mime_type: mime_type.clone(),
        size_bytes,
        uri: dest.to_string_lossy().into_owned(),
        metadata: json!({
            "location": LOCATION_LOCAL,
            "provider": "local",
            "source_path": target.to_string_lossy(),
        }),
    };

    let inline_content = (size_bytes as u64 <= INLINE_CONTENT_LIMIT)
        .then(|| String::from_utf8(bytes).ok())
        .flatten();

    let mut output = json!({
        "path": params.path,
        "size_bytes": size_bytes,
        "mime_type": mime_type,
        "location": LOCATION_LOCAL,
    });
    if let (Some(content), Some(object)) = (inline_content, output.as_object_mut()) {
        object.insert("content".to_string(), json!(content));
    }

    Ok(TaskExecutionResult {
        message: Some(format!(
            "Read {size_bytes} byte(s) from {}",
            target.display()
        )),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: vec![artifact],
    })
}

fn write_file(
    request: &ProviderExecutionRequest,
    root: &Path,
) -> Result<TaskExecutionResult, SendableError> {
    if !sandbox::writes_allowed() {
        return Err(WRITE_DISABLED.error("write_file"));
    }
    let params: WriteParams = parse_params(request)?;
    let target = sandbox::resolve_for_write(root, &params.path)?;
    std::fs::write(&target, params.content.as_bytes())
        .map_err(|err| IO.error(format!("{}: {err}", params.path)))?;
    let size_bytes = params.content.len() as i64;

    Ok(TaskExecutionResult {
        message: Some(format!(
            "Wrote {size_bytes} byte(s) to {}",
            target.display()
        )),
        output_json: Some(json!({
            "path": params.path,
            "size_bytes": size_bytes,
            "location": LOCATION_LOCAL,
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn list_dir(
    request: &ProviderExecutionRequest,
    root: &Path,
) -> Result<TaskExecutionResult, SendableError> {
    let params: PathParams = parse_params(request)?;
    let target = sandbox::resolve_existing(root, &params.path)?;
    if !target.is_dir() {
        return Err(NOT_A_DIRECTORY.error(&params.path));
    }

    let mut entries = Vec::new();
    for entry in
        std::fs::read_dir(&target).map_err(|err| IO.error(format!("{}: {err}", params.path)))?
    {
        let entry = entry.map_err(|err| IO.error(format!("{}: {err}", params.path)))?;
        let metadata = entry.metadata().ok();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "is_dir": metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            "size_bytes": metadata.as_ref().map(|m| m.len() as i64).unwrap_or(0),
        }));
    }

    Ok(TaskExecutionResult {
        message: Some(format!(
            "Listed {} entr(ies) in {}",
            entries.len(),
            target.display()
        )),
        output_json: Some(json!({
            "path": params.path,
            "entries": entries,
            "location": LOCATION_LOCAL,
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn stat(
    request: &ProviderExecutionRequest,
    root: &Path,
) -> Result<TaskExecutionResult, SendableError> {
    let params: PathParams = parse_params(request)?;
    let resolved = sandbox::resolve_optional(root, &params.path)?;

    let output = match resolved {
        Some(path) => {
            let metadata = std::fs::metadata(&path)
                .map_err(|err| IO.error(format!("{}: {err}", params.path)))?;
            json!({
                "path": params.path,
                "exists": true,
                "is_dir": metadata.is_dir(),
                "size_bytes": metadata.len() as i64,
                "location": LOCATION_LOCAL,
            })
        }
        None => json!({
            "path": params.path,
            "exists": false,
            "location": LOCATION_LOCAL,
        }),
    };

    Ok(TaskExecutionResult {
        message: None,
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn delete(
    request: &ProviderExecutionRequest,
    root: &Path,
) -> Result<TaskExecutionResult, SendableError> {
    if !sandbox::writes_allowed() {
        return Err(WRITE_DISABLED.error("delete"));
    }
    let params: PathParams = parse_params(request)?;
    let target = sandbox::resolve_existing(root, &params.path)?;
    if !target.is_file() {
        return Err(NOT_A_FILE.error(&params.path));
    }
    std::fs::remove_file(&target).map_err(|err| IO.error(format!("{}: {err}", params.path)))?;

    Ok(TaskExecutionResult {
        message: Some(format!("Deleted {}", target.display())),
        output_json: Some(json!({
            "path": params.path,
            "deleted": true,
            "location": LOCATION_LOCAL,
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
