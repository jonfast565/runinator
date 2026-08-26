use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use runinator_models::bundles::SecretBundle;
use runinator_models::functions::FunctionCatalogEntry;
use runinator_models::pipelines::PipelineBundle;
use runinator_models::providers::ProviderMetadata;
use runinator_models::semver::SemVer;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition};
use runinator_rexrap::WorkflowSignature;

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

fn parse_pack_source(path: &Path, data: &str) -> Result<runinator_rexrap::RrxBlocks> {
    let blocks = runinator_rexrap::parse_rrx_blocks(data).map_err(|e| {
        PackError::compile(format!(
            "failed to parse {}:\n{}",
            path.display(),
            e.render(data)
        ))
    })?;
    if !blocks.language_header {
        return Err(PackError::compile(format!(
            "{} must begin with `language rexrap-1`; legacy headerless pack sources are not accepted",
            path.display()
        )));
    }
    Ok(blocks)
}

// Returns true when the path is a unified `.rrx` source or a directory pack.
// rather than a raw workflow/bundle json file.
pub fn is_pack_source(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("rrx"))
}

// list source files that make up a pack so dev mode can detect changes without compiling the pack.
pub fn pack_source_files(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if path.is_dir() {
        files.extend(rexrap_directory_paths(path)?);
        extend_rexrap_includes(&mut files);
        if let Some(settings_path) = pack_settings_path(path)? {
            files.push(settings_path);
        }
        files.extend(pack_pipeline_paths(path)?);
        files.sort();
        files.dedup();
        return Ok(files);
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rrx") => {
            files.push(path.to_path_buf());
            extend_rexrap_includes(&mut files);
        }
        _ => files.push(path.to_path_buf()),
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn extend_rexrap_includes(files: &mut Vec<PathBuf>) {
    let rexrap_files = files
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rrx"))
        .cloned()
        .collect::<Vec<_>>();
    for path in rexrap_files {
        let Ok(data) = fs::read_to_string(&path) else {
            continue;
        };
        let source_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let Ok(included) = runinator_rexrap::included_file_paths(&data, source_dir) else {
            continue;
        };
        files.extend(included);
    }
}

// load a settings bundle that ships alongside a pack source: a `.rexrapm` manifest's optional
// "settings" path entry, or a sibling `settings.rexraps`/`settings.json` next to a directory pack. a
// `.rexraps` settings file is parsed with the rexrap secrets front end; `.json` is read directly. a
// single .rexrap or a pack without a settings file yields None.
pub fn load_pack_settings(path: &Path) -> Result<Option<SecretBundle>> {
    let paths = if path.is_dir() {
        rexrap_directory_paths(path)?
    } else {
        vec![path.to_path_buf()]
    };
    let mut secrets = Vec::new();
    for source_path in paths {
        let data = fs::read_to_string(&source_path)?;
        let blocks = parse_pack_source(&source_path, &data)?;
        for settings in blocks.settings {
            let mut bundle = runinator_rexrap::parse_secrets_str(&settings).map_err(|e| {
                PackError::compile(format!(
                    "failed to parse {} settings:\n{}",
                    source_path.display(),
                    e.render(&settings)
                ))
            })?;
            if let Some(modified) = file_modified(&source_path) {
                for entry in &mut bundle.secrets {
                    entry.updated_at.get_or_insert(modified);
                }
            }
            secrets.extend(bundle.secrets);
        }
    }
    Ok((!secrets.is_empty()).then_some(SecretBundle { secrets }))
}

// load pipeline declarations that ship with a pack: a `.rexrapm` manifest's optional "pipelines" array
// of relative `.rexrapp` paths, or any `*.rexrapp` files next to a directory pack. each is parsed with the
// rexrap pipeline front end and merged into one bundle. a single `.rexrap` or a pack without pipeline files
// yields None.
pub fn load_pack_pipelines(path: &Path) -> Result<Option<PipelineBundle>> {
    let paths = if path.is_dir() {
        rexrap_directory_paths(path)?
    } else {
        vec![path.to_path_buf()]
    };
    let mut pipelines = Vec::new();
    for source_path in paths {
        let data = fs::read_to_string(&source_path)?;
        let blocks = parse_pack_source(&source_path, &data)?;
        if !blocks.pipelines.trim().is_empty() {
            let bundle = runinator_rexrap::parse_pipeline_str(&blocks.pipelines).map_err(|e| {
                PackError::compile(format!(
                    "failed to parse {} pipelines:\n{}",
                    source_path.display(),
                    e.render(&blocks.pipelines)
                ))
            })?;
            for pipeline in &bundle.pipelines {
                if pipeline.key.is_none() {
                    return Err(PackError::compile(format!(
                        "pipeline '{}' in {} must declare a stable `key`",
                        pipeline.name,
                        source_path.display()
                    )));
                }
                if pipeline.namespace.is_none() {
                    return Err(PackError::compile(format!(
                        "pipeline '{}' in {} must declare a `namespace`",
                        pipeline.name,
                        source_path.display()
                    )));
                }
            }
            pipelines.extend(bundle.pipelines);
        }
    }
    Ok((!pipelines.is_empty()).then_some(PipelineBundle { pipelines }))
}

// resolve the pipeline file paths for a pack source: a directory pack's `*.rexrapp` files, or a `.rexrapm`
// manifest's optional "pipelines" array of relative paths. anything else yields an empty list.
fn pack_pipeline_paths(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_dir() {
        let mut paths = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry_path = entry?.path();
            if entry_path.extension().and_then(|ext| ext.to_str()) == Some("rexrapp") {
                paths.push(entry_path);
            }
        }
        paths.sort();
        return Ok(paths);
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("rexrapm") {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&data)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut paths = Vec::new();
    if let Some(entries) = manifest.get("pipelines").and_then(Value::as_array) {
        for entry in entries {
            let rel = entry
                .as_str()
                .or_else(|| entry.get("path").and_then(Value::as_str))
                .ok_or_else(|| {
                    PackError::source(
                        "each manifest pipeline entry must be a path string or have a 'path'",
                    )
                })?;
            paths.push(base_dir.join(rel));
        }
    }
    paths.sort();
    Ok(paths)
}

// resolve the settings file path for a pack source, if one exists. a directory pack prefers a
// `settings.rexraps` over a `settings.json`.
fn pack_settings_path(path: &Path) -> Result<Option<PathBuf>> {
    if path.is_dir() {
        for name in ["settings.rexraps", "settings.json"] {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
        return Ok(None);
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("rexrapm") {
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

// compile a rexrap pack source into a workflow bundle: a single .rexrap, a .rexrapm manifest, or a
// directory of .rexrap files.
/// what a pack is compiled against: provider metadata plus published packaged-function exports.
///
/// one struct rather than two parameters because the two travel together everywhere and a compile
/// given only one of them silently loses the ability to resolve half its calls.
#[derive(Debug, Clone, Default)]
pub struct PackCatalog {
    pub providers: Vec<ProviderMetadata>,
    pub functions: Vec<FunctionCatalogEntry>,
}

impl PackCatalog {
    pub fn with_providers(providers: &[ProviderMetadata]) -> Self {
        Self {
            providers: providers.to_vec(),
            functions: Vec::new(),
        }
    }
}

pub fn load_workflow_bundle(path: &Path) -> Result<WorkflowBundle> {
    load_workflow_bundle_with_catalog(path, &PackCatalog::default())
}

// compile a rexrap pack source with supplemental provider metadata. built-in provider metadata is
// always included so local/offline pack compilation matches the worker's built-in action catalog.
pub fn load_workflow_bundle_with_providers(
    path: &Path,
    providers: &[ProviderMetadata],
) -> Result<WorkflowBundle> {
    load_workflow_bundle_with_catalog(path, &PackCatalog::with_providers(providers))
}

/// compile a pack against provider metadata *and* the published packaged-function catalog.
pub fn load_workflow_bundle_with_catalog(
    path: &Path,
    catalog: &PackCatalog,
) -> Result<WorkflowBundle> {
    if path.is_dir() {
        return load_rexrap_directory(path, catalog);
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rrx") => {
            let data = fs::read_to_string(path)?;
            let blocks = parse_pack_source(path, &data)?;
            Ok(WorkflowBundle {
                workflows: compile_rexrap_all_with_signatures(
                    path,
                    &blocks.workflows,
                    SemVer::default(),
                    catalog,
                    &[],
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

// format and compile one .rexrap source into a definition.
// imported workflows are enabled so a pack is live as soon as it lands.
pub fn compile_rexrap(
    path: &Path,
    data: &str,
    default_version: SemVer,
) -> Result<WorkflowDefinition> {
    compile_rexrap_with_providers(path, data, default_version, &[])
}

pub fn compile_rexrap_with_providers(
    path: &Path,
    data: &str,
    default_version: SemVer,
    providers: &[ProviderMetadata],
) -> Result<WorkflowDefinition> {
    compile_rexrap_with_signatures(
        path,
        data,
        default_version,
        &PackCatalog::with_providers(providers),
        &[],
    )
}

pub fn compile_rexrap_all_with_providers(
    path: &Path,
    data: &str,
    default_version: SemVer,
    providers: &[ProviderMetadata],
) -> Result<Vec<WorkflowDefinition>> {
    compile_rexrap_all_with_signatures(
        path,
        data,
        default_version,
        &PackCatalog::with_providers(providers),
        &[],
    )
}

fn compile_rexrap_with_signatures(
    path: &Path,
    data: &str,
    default_version: SemVer,
    catalog: &PackCatalog,
    workflow_signatures: &[WorkflowSignature],
) -> Result<WorkflowDefinition> {
    let options = runinator_rexrap::CompileOptions {
        enabled: true,
        default_version,
        source_dir: path.parent().map(Path::to_path_buf),
        providers: compile_providers(&catalog.providers),
        strict_namespaces: true,
        functions: catalog.functions.clone(),
        workflow_signatures: workflow_signatures.to_vec(),
        ..runinator_rexrap::CompileOptions::default()
    };
    let formatted = runinator_rexrap::format_str(data).map_err(|e| {
        PackError::compile(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            e.render(data)
        ))
    })?;
    let mut definition = runinator_rexrap::compile_str(&formatted, &options).map_err(|e| {
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

fn compile_rexrap_all_with_signatures(
    path: &Path,
    data: &str,
    default_version: SemVer,
    catalog: &PackCatalog,
    workflow_signatures: &[WorkflowSignature],
) -> Result<Vec<WorkflowDefinition>> {
    let options = runinator_rexrap::CompileOptions {
        enabled: true,
        default_version,
        source_dir: path.parent().map(Path::to_path_buf),
        providers: compile_providers(&catalog.providers),
        strict_namespaces: true,
        functions: catalog.functions.clone(),
        workflow_signatures: workflow_signatures.to_vec(),
        ..runinator_rexrap::CompileOptions::default()
    };
    let formatted = runinator_rexrap::format_str(data).map_err(|e| {
        PackError::compile(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            e.render(data)
        ))
    })?;
    let mut definitions = runinator_rexrap::compile_all_str(&formatted, &options).map_err(|e| {
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
        let blocks = runinator_rexrap::parse_rrx_blocks(source).map_err(|e| {
            PackError::compile(format!(
                "failed to read unified source {}:\n{}",
                path.display(),
                e.render(source)
            ))
        })?;
        if blocks.workflows.trim().is_empty() {
            continue;
        }
        let mut source_signatures =
            runinator_rexrap::workflow_signature_from_source(&blocks.workflows).map_err(|e| {
                PackError::compile(format!(
                    "failed to read workflow signature from {}:\n{}",
                    path.display(),
                    e.render(&blocks.workflows)
                ))
            })?;
        signatures.append(&mut source_signatures);
    }
    Ok(signatures)
}

/// collect sibling workflow signatures for strict single-file rexrap tooling.
pub fn rexrap_context_workflow_signatures(
    path: &Path,
    current_source: Option<&str>,
) -> Result<Vec<WorkflowSignature>> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("rrx") {
        return Ok(Vec::new());
    }

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut paths = rexrap_directory_paths(dir)?;
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

// compile every unified `.rrx` source in a directory (sorted for deterministic ids) into one bundle.
fn load_rexrap_directory(dir: &Path, catalog: &PackCatalog) -> Result<WorkflowBundle> {
    let rexrap_paths = rexrap_directory_paths(dir)?;
    if rexrap_paths.is_empty() {
        return Err(PackError::source(format!(
            "no .rrx files found in {}",
            dir.display()
        )));
    }

    let mut combined = String::from("language rexrap-1\n\n");
    let mut newest = None;
    for rexrap_path in &rexrap_paths {
        let data = fs::read_to_string(rexrap_path)?;
        let blocks = parse_pack_source(rexrap_path, &data)?;
        if !blocks.workflows.trim().is_empty() {
            for line in blocks.workflows.lines() {
                if line.trim() != "language rexrap-1" {
                    combined.push_str(line);
                    combined.push('\n');
                }
            }
            combined.push('\n');
        }
        newest = newest.max(file_modified(rexrap_path));
    }
    let synthetic = dir.join("__pack__.rrx");
    let signatures = runinator_rexrap::workflow_signature_from_source(&combined).map_err(|e| {
        PackError::compile(format!(
            "failed to read pack workflow signatures from {}:\n{}",
            dir.display(),
            e.render(&combined)
        ))
    })?;
    let mut workflows = compile_rexrap_all_with_signatures(
        &synthetic,
        &combined,
        SemVer::default(),
        catalog,
        &signatures,
    )?;
    for workflow in &mut workflows {
        workflow.updated_at = newest;
    }
    Ok(WorkflowBundle {
        workflows,
        triggers: Vec::new(),
    })
}

fn rexrap_directory_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut rexrap_paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry_path = entry?.path();
        if entry_path.is_dir() {
            rexrap_paths.extend(rexrap_directory_paths(&entry_path)?);
        } else if entry_path.extension().and_then(|ext| ext.to_str()) == Some("rrx") {
            rexrap_paths.push(entry_path);
        }
    }
    rexrap_paths.sort();
    Ok(rexrap_paths)
}
