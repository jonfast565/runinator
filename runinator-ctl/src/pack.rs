use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use runinator_models::bundles::SecretBundle;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger};

use crate::commands::{Result, err};

// the source file's last-modified time, used to stamp compiled artifacts so re-applying an edited
// pack overwrites the stored copy (newer mtime wins) while an unedited file is skipped.
fn file_modified(path: &Path) -> Option<DateTime<Utc>> {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

#[cfg(test)]
mod tests;

// returns true when the path is a wdl pack source (a directory, a .wdl, or a .wdlp manifest)
// rather than a raw workflow/bundle json file.
pub fn is_pack_source(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("wdl") | Some("wdlp")
    )
}

// list source files that make up a pack so dev mode can detect changes without compiling the pack.
pub fn pack_source_files(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if path.is_dir() {
        files.extend(wdl_directory_paths(path)?);
        if let Some(settings_path) = pack_settings_path(path)? {
            files.push(settings_path);
        }
        files.sort();
        files.dedup();
        return Ok(files);
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdlp") => {
            files.push(path.to_path_buf());
            files.extend(wdl_pack_manifest_paths(path)?);
            if let Some(settings_path) = pack_settings_path(path)? {
                files.push(settings_path);
            }
        }
        Some("wdl") => files.push(path.to_path_buf()),
        _ => files.push(path.to_path_buf()),
    }

    files.sort();
    files.dedup();
    Ok(files)
}

// load a settings bundle that ships alongside a pack source: a `.wdlp` manifest's optional
// "settings" path entry, or a sibling `settings.wdls`/`settings.json` next to a directory pack. a
// `.wdls` settings file is parsed with the wdl secrets front end; `.json` is read directly. a
// single .wdl or a pack without a settings file yields None.
pub fn load_pack_settings(path: &Path) -> Result<Option<SecretBundle>> {
    let Some(settings_path) = pack_settings_path(path)? else {
        return Ok(None);
    };
    parse_settings_file(&settings_path).map(Some)
}

// parse a settings file, choosing the `.wdls` secrets front end or json by extension.
fn parse_settings_file(path: &Path) -> Result<SecretBundle> {
    let data = fs::read_to_string(path)?;
    let mut bundle: SecretBundle = match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdls") => runinator_wdl::parse_secrets_str(&data).map_err(|e| {
            err(format!(
                "failed to parse {}:\n{}",
                path.display(),
                e.render(&data)
            ))
        })?,
        _ => serde_json::from_str(&data)?,
    };
    // stamp entries that did not declare their own time, so re-import reconciles by file mtime.
    if let Some(modified) = file_modified(path) {
        for entry in &mut bundle.secrets {
            entry.updated_at.get_or_insert(modified);
        }
    }
    Ok(bundle)
}

// resolve the settings file path for a pack source, if one exists. a directory pack prefers a
// `settings.wdls` over a `settings.json`.
fn pack_settings_path(path: &Path) -> Result<Option<PathBuf>> {
    if path.is_dir() {
        for name in ["settings.wdls", "settings.json"] {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
        return Ok(None);
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("wdlp") {
        return Ok(None);
    }
    let data = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&data)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(manifest
        .get("settings")
        .and_then(Value::as_str)
        .map(|rel| base_dir.join(rel)))
}

// compile a wdl pack source into a workflow bundle: a single .wdl, a .wdlp manifest, or a
// directory of .wdl files.
pub fn load_workflow_bundle(path: &Path) -> Result<WorkflowBundle> {
    if path.is_dir() {
        return load_wdl_directory(path);
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdlp") => load_wdl_pack_manifest(path),
        Some("wdl") => {
            let data = fs::read_to_string(path)?;
            let definition = compile_wdl(path, &data, 1)?;
            Ok(WorkflowBundle {
                workflows: vec![definition],
                triggers: Vec::new(),
            })
        }
        _ => Err(err(format!("unsupported pack source: {}", path.display()))),
    }
}

// format and compile one .wdl source into a definition.
// imported workflows are enabled so a pack is live as soon as it lands.
fn compile_wdl(path: &Path, data: &str, default_version: i64) -> Result<WorkflowDefinition> {
    let options = runinator_wdl::CompileOptions {
        enabled: true,
        default_version,
    };
    let formatted = runinator_wdl::format_str(data).map_err(|e| {
        err(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            e.render(data)
        ))
    })?;
    let mut definition = runinator_wdl::compile_str(&formatted, &options).map_err(|e| {
        err(format!(
            "failed to compile {}:\n{}",
            path.display(),
            e.render(&formatted)
        ))
    })?;
    // stamp with the source mtime so re-applying an edited file overwrites the stored workflow.
    definition.updated_at = file_modified(path);
    Ok(definition)
}

// compile every *.wdl in a directory (sorted for deterministic ids) into one bundle.
fn load_wdl_directory(dir: &Path) -> Result<WorkflowBundle> {
    let wdl_paths = wdl_directory_paths(dir)?;
    if wdl_paths.is_empty() {
        return Err(err(format!("no .wdl files found in {}", dir.display())));
    }

    let mut workflows = Vec::with_capacity(wdl_paths.len());
    for wdl_path in &wdl_paths {
        let data = fs::read_to_string(wdl_path)?;
        workflows.push(compile_wdl(wdl_path, &data, 1)?);
    }
    Ok(WorkflowBundle {
        workflows,
        triggers: Vec::new(),
    })
}

fn wdl_directory_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut wdl_paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry_path = entry?.path();
        if entry_path.extension().and_then(|ext| ext.to_str()) == Some("wdl") {
            wdl_paths.push(entry_path);
        }
    }
    wdl_paths.sort();
    Ok(wdl_paths)
}

// resolve a .wdlp manifest: compile each referenced .wdl (relative to the manifest) and
// pass through any declared triggers.
fn load_wdl_pack_manifest(path: &Path) -> Result<WorkflowBundle> {
    let data = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&data)?;
    let version = manifest
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(1);

    let entries = wdl_pack_manifest_paths_from_value(path, &manifest)?;

    let mut workflows = Vec::with_capacity(entries.len());
    for wdl_path in entries {
        let source = fs::read_to_string(&wdl_path)?;
        workflows.push(compile_wdl(&wdl_path, &source, version)?);
    }

    let triggers = match manifest.get("triggers").cloned() {
        Some(value) if !value.is_null() => {
            serde_json::from_value::<Vec<WorkflowTrigger>>(value.into())?
        }
        _ => Vec::new(),
    };

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}

fn wdl_pack_manifest_paths(path: &Path) -> Result<Vec<PathBuf>> {
    let data = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&data)?;
    wdl_pack_manifest_paths_from_value(path, &manifest)
}

fn wdl_pack_manifest_paths_from_value(path: &Path, manifest: &Value) -> Result<Vec<PathBuf>> {
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entries = manifest
        .get("workflows")
        .and_then(Value::as_array)
        .ok_or_else(|| err("wdl pack manifest missing 'workflows' array"))?;

    let mut paths = Vec::with_capacity(entries.len());
    for entry in entries {
        let rel = entry
            .as_str()
            .or_else(|| entry.get("path").and_then(Value::as_str))
            .ok_or_else(|| {
                err("each manifest workflow entry must be a path string or have a 'path'")
            })?;
        paths.push(base_dir.join(rel));
    }
    paths.sort();
    Ok(paths)
}
