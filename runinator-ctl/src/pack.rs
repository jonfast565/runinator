use std::fs;
use std::path::Path;

use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger};

use crate::commands::{Result, err};

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
    runinator_wdl::compile_str(&formatted, &options).map_err(|e| {
        err(format!(
            "failed to compile {}:\n{}",
            path.display(),
            e.render(&formatted)
        ))
    })
}

// compile every *.wdl in a directory (sorted for deterministic ids) into one bundle.
fn load_wdl_directory(dir: &Path) -> Result<WorkflowBundle> {
    let mut wdl_paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry_path = entry?.path();
        if entry_path.extension().and_then(|ext| ext.to_str()) == Some("wdl") {
            wdl_paths.push(entry_path);
        }
    }
    wdl_paths.sort();
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

// resolve a .wdlp manifest: compile each referenced .wdl (relative to the manifest) and
// pass through any declared triggers.
fn load_wdl_pack_manifest(path: &Path) -> Result<WorkflowBundle> {
    let data = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&data)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));

    let version = manifest
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(1);

    let entries = manifest
        .get("workflows")
        .and_then(Value::as_array)
        .ok_or_else(|| err("wdl pack manifest missing 'workflows' array"))?;

    let mut workflows = Vec::with_capacity(entries.len());
    for entry in entries {
        let rel = entry
            .as_str()
            .or_else(|| entry.get("path").and_then(Value::as_str))
            .ok_or_else(|| {
                err("each manifest workflow entry must be a path string or have a 'path'")
            })?;
        let wdl_path = base_dir.join(rel);
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
