use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use runinator_models::bundles::SecretBundle;
use runinator_models::providers::ProviderMetadata;
use runinator_models::semver::SemVer;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger};
use runinator_wdl::WorkflowSignature;

use crate::errors::{PackError, Result};

#[cfg(test)]
mod tests;

// the source file's last-modified time, used to stamp compiled artifacts so re-applying an edited
// pack overwrites the stored copy (newer mtime wins) while an unedited file is skipped.
fn file_modified(path: &Path) -> Option<DateTime<Utc>> {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

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
        extend_wdl_includes(&mut files);
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
            extend_wdl_includes(&mut files);
            if let Some(settings_path) = pack_settings_path(path)? {
                files.push(settings_path);
            }
        }
        Some("wdl") => {
            files.push(path.to_path_buf());
            extend_wdl_includes(&mut files);
        }
        _ => files.push(path.to_path_buf()),
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn extend_wdl_includes(files: &mut Vec<PathBuf>) {
    let wdl_files = files
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wdl"))
        .cloned()
        .collect::<Vec<_>>();
    for path in wdl_files {
        let Ok(data) = fs::read_to_string(&path) else {
            continue;
        };
        let source_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let Ok(included) = runinator_wdl::included_file_paths(&data, source_dir) else {
            continue;
        };
        files.extend(included);
    }
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
            PackError::compile(format!(
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
    load_workflow_bundle_with_providers(path, &[])
}

// compile a wdl pack source with supplemental provider metadata. built-in provider metadata is
// always included so local/offline pack compilation matches the worker's built-in action catalog.
pub fn load_workflow_bundle_with_providers(
    path: &Path,
    providers: &[ProviderMetadata],
) -> Result<WorkflowBundle> {
    if path.is_dir() {
        return load_wdl_directory(path, providers);
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdlp") => load_wdl_pack_manifest(path, providers),
        Some("wdl") => {
            let data = fs::read_to_string(path)?;
            Ok(WorkflowBundle {
                workflows: compile_wdl_all_with_providers(
                    path,
                    &data,
                    SemVer::default(),
                    providers,
                )?,
                triggers: Vec::new(),
            })
        }
        _ => Err(PackError::source(format!(
            "unsupported pack source: {}",
            path.display()
        ))),
    }
}

// format and compile one .wdl source into a definition.
// imported workflows are enabled so a pack is live as soon as it lands.
pub fn compile_wdl(path: &Path, data: &str, default_version: SemVer) -> Result<WorkflowDefinition> {
    compile_wdl_with_providers(path, data, default_version, &[])
}

pub fn compile_wdl_with_providers(
    path: &Path,
    data: &str,
    default_version: SemVer,
    providers: &[ProviderMetadata],
) -> Result<WorkflowDefinition> {
    compile_wdl_with_signatures(path, data, default_version, providers, &[])
}

pub fn compile_wdl_all_with_providers(
    path: &Path,
    data: &str,
    default_version: SemVer,
    providers: &[ProviderMetadata],
) -> Result<Vec<WorkflowDefinition>> {
    compile_wdl_all_with_signatures(path, data, default_version, providers, &[])
}

fn compile_wdl_with_signatures(
    path: &Path,
    data: &str,
    default_version: SemVer,
    providers: &[ProviderMetadata],
    workflow_signatures: &[WorkflowSignature],
) -> Result<WorkflowDefinition> {
    let options = runinator_wdl::CompileOptions {
        enabled: true,
        default_version,
        source_dir: path.parent().map(Path::to_path_buf),
        providers: compile_providers(providers),
        workflow_signatures: workflow_signatures.to_vec(),
        ..runinator_wdl::CompileOptions::default()
    };
    let formatted = runinator_wdl::format_str(data).map_err(|e| {
        PackError::compile(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            e.render(data)
        ))
    })?;
    let mut definition = runinator_wdl::compile_str(&formatted, &options).map_err(|e| {
        PackError::compile(format!(
            "failed to compile {}:\n{}",
            path.display(),
            e.render(&formatted)
        ))
    })?;
    // stamp with the source mtime so re-applying an edited file overwrites the stored workflow.
    definition.updated_at = file_modified(path);
    Ok(definition)
}

fn compile_wdl_all_with_signatures(
    path: &Path,
    data: &str,
    default_version: SemVer,
    providers: &[ProviderMetadata],
    workflow_signatures: &[WorkflowSignature],
) -> Result<Vec<WorkflowDefinition>> {
    let options = runinator_wdl::CompileOptions {
        enabled: true,
        default_version,
        source_dir: path.parent().map(Path::to_path_buf),
        providers: compile_providers(providers),
        workflow_signatures: workflow_signatures.to_vec(),
        ..runinator_wdl::CompileOptions::default()
    };
    let formatted = runinator_wdl::format_str(data).map_err(|e| {
        PackError::compile(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            e.render(data)
        ))
    })?;
    let mut definitions = runinator_wdl::compile_all_str(&formatted, &options).map_err(|e| {
        PackError::compile(format!(
            "failed to compile {}:\n{}",
            path.display(),
            e.render(&formatted)
        ))
    })?;
    for definition in &mut definitions {
        definition.updated_at = file_modified(path);
    }
    Ok(definitions)
}

fn collect_workflow_signatures(paths: &[PathBuf]) -> Result<Vec<WorkflowSignature>> {
    collect_workflow_signatures_with_current(paths, None, None)
}

fn collect_workflow_signatures_with_current(
    paths: &[PathBuf],
    current_path: Option<&Path>,
    current_source: Option<&str>,
) -> Result<Vec<WorkflowSignature>> {
    let mut signatures = Vec::new();
    for path in paths {
        let data;
        let source = if Some(path.as_path()) == current_path {
            match current_source {
                Some(source) => source,
                None => {
                    data = fs::read_to_string(path)?;
                    &data
                }
            }
        } else {
            data = fs::read_to_string(path)?;
            &data
        };
        let mut source_signatures =
            runinator_wdl::workflow_signature_from_source(source).map_err(|e| {
                PackError::compile(format!(
                    "failed to read workflow signature from {}:\n{}",
                    path.display(),
                    e.render(source)
                ))
            })?;
        signatures.append(&mut source_signatures);
    }
    Ok(signatures)
}

/// collect sibling workflow signatures for strict single-file wdl tooling.
pub fn wdl_context_workflow_signatures(
    path: &Path,
    current_source: Option<&str>,
) -> Result<Vec<WorkflowSignature>> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("wdl") {
        return Ok(Vec::new());
    }

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut paths = wdl_directory_paths(dir)?;
    if !paths.iter().any(|candidate| candidate == path) {
        paths.push(path.to_path_buf());
        paths.sort();
    }
    collect_workflow_signatures_with_current(&paths, Some(path), current_source)
}

fn compile_providers(providers: &[ProviderMetadata]) -> Vec<ProviderMetadata> {
    let mut merged = std::collections::BTreeMap::new();
    for provider in runinator_provider_catalog::metadata() {
        merged.insert(provider.name.clone(), provider);
    }
    for provider in providers {
        merged.insert(provider.name.clone(), provider.clone());
    }
    merged.into_values().collect()
}

// compile every *.wdl in a directory (sorted for deterministic ids) into one bundle.
fn load_wdl_directory(dir: &Path, providers: &[ProviderMetadata]) -> Result<WorkflowBundle> {
    let wdl_paths = wdl_directory_paths(dir)?;
    if wdl_paths.is_empty() {
        return Err(PackError::source(format!(
            "no .wdl files found in {}",
            dir.display()
        )));
    }

    let workflow_signatures = collect_workflow_signatures(&wdl_paths)?;
    let mut workflows = Vec::with_capacity(wdl_paths.len());
    for wdl_path in &wdl_paths {
        let data = fs::read_to_string(wdl_path)?;
        workflows.extend(compile_wdl_all_with_signatures(
            wdl_path,
            &data,
            SemVer::default(),
            providers,
            &workflow_signatures,
        )?);
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
fn load_wdl_pack_manifest(path: &Path, providers: &[ProviderMetadata]) -> Result<WorkflowBundle> {
    let data = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&data)?;
    let version = manifest
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<SemVer>().ok())
                .or_else(|| {
                    v.as_i64()
                        .map(|major| SemVer::new(major.max(0) as u64, 0, 0))
                })
        })
        .unwrap_or_default();

    let entries = wdl_pack_manifest_paths_from_value(path, &manifest)?;

    let workflow_signatures = collect_workflow_signatures(&entries)?;
    let mut workflows = Vec::with_capacity(entries.len());
    for wdl_path in entries {
        let source = fs::read_to_string(&wdl_path)?;
        workflows.extend(compile_wdl_all_with_signatures(
            &wdl_path,
            &source,
            version,
            providers,
            &workflow_signatures,
        )?);
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
        .ok_or_else(|| PackError::source("wdl pack manifest missing 'workflows' array"))?;

    let mut paths = Vec::with_capacity(entries.len());
    for entry in entries {
        let rel = entry
            .as_str()
            .or_else(|| entry.get("path").and_then(Value::as_str))
            .ok_or_else(|| {
                PackError::source(
                    "each manifest workflow entry must be a path string or have a 'path'",
                )
            })?;
        paths.push(base_dir.join(rel));
    }
    paths.sort();
    Ok(paths)
}
