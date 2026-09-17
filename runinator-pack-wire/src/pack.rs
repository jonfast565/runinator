// shared layout for the compiled workflow pack uploaded to the web service. the client compiles a
// pack (`.rexrap`/`.rexraps`/`.rexrapm`) and zips the resulting json artifacts; the web service unzips and
// imports them. compilation stays on the client — the backend only reads the compiled json here.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use runinator_models::bundles::SettingsBundle;
use runinator_models::functions::NewFunctionVersion;
use runinator_models::pipelines::PipelineBundle;
use runinator_models::workflows::WorkflowBundle;
use zip::write::SimpleFileOptions;

/// zip entry holding the compiled `WorkflowBundle` json (always present).
pub const WORKFLOWS_ENTRY: &str = "workflows.json";
/// zip entry holding the compiled versioned `SettingsBundle` json (optional).
pub const SETTINGS_ENTRY: &str = "settings.json";
/// Legacy settings entry accepted for one compatibility release.
pub const SECRETS_ENTRY: &str = "secrets.json";
/// zip entry holding the compiled `PipelineBundle` json (optional).
pub const PIPELINES_ENTRY: &str = "pipelines.json";
/// zip entry holding the packaged-function publish requests (optional).
pub const FUNCTIONS_ENTRY: &str = "functions.json";
/// prefix under which a pack carries function archives, one entry per digest.
///
/// entries are named `function-artifacts/<sha256-hex>.zip`, so the digest is recoverable from the
/// entry name alone and the reader never has to trust a manifest to tell it what bytes it has.
pub const FUNCTION_ARTIFACT_PREFIX: &str = "function-artifacts/";

/// Maximum number of central-directory entries accepted from one pack.
pub const MAX_PACK_ENTRIES: usize = 1024;
/// Maximum uncompressed bytes accepted from any single pack entry.
pub const MAX_PACK_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum aggregate uncompressed size of a pack, including entries this version does not use.
pub const MAX_PACK_UNCOMPRESSED_BYTES: u64 = 32 * 1024 * 1024;

/// error type for pack zip read/write; boxes zip and serde failures alike.
pub type PackError = Box<dyn std::error::Error + Send + Sync>;

/// what a pack zip carries once read back.

/// build a compiled pack zip from a workflow bundle and optional secret / pipeline bundles.
///
/// kept for the callers that carry nothing else; anything richer goes through [`PackBuilder`].
pub fn build_pack_zip(
    workflows: &WorkflowBundle,
    settings: Option<&SettingsBundle>,
    pipelines: Option<&PipelineBundle>,
) -> Result<Vec<u8>, PackError> {
    PackBuilder::new(workflows)
        .settings(settings)
        .pipelines(pipelines)
        .build()
}

/// read a compiled pack zip back into its workflow bundle and optional secret / pipeline bundles.
pub fn read_pack_zip(bytes: &[u8]) -> Result<PackContents, PackError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    validate_archive_layout(&mut archive)?;
    let mut budget = ReadBudget::default();
    let workflows: WorkflowBundle = {
        let mut file = archive
            .by_name(WORKFLOWS_ENTRY)
            .map_err(|_| format!("pack zip missing '{WORKFLOWS_ENTRY}'"))?;
        let bytes = budget.read(&mut file, WORKFLOWS_ENTRY)?;
        serde_json::from_slice(&bytes)?
    };
    let settings = read_optional_entry(&mut archive, SETTINGS_ENTRY, &mut budget)?;
    let legacy_settings = read_optional_entry(&mut archive, SECRETS_ENTRY, &mut budget)?;
    if settings.is_some() && legacy_settings.is_some() {
        return Err(format!(
            "pack zip cannot contain both '{SETTINGS_ENTRY}' and legacy '{SECRETS_ENTRY}'"
        )
        .into());
    }
    let settings = settings.or(legacy_settings);
    let pipelines = read_optional_entry(&mut archive, PIPELINES_ENTRY, &mut budget)?;
    let functions =
        read_optional_entry(&mut archive, FUNCTIONS_ENTRY, &mut budget)?.unwrap_or_default();

    // artifacts are enumerated by prefix rather than looked up by name: the reader does not know
    // which digests a pack carries until it looks, and the entry name is where the digest comes
    // from. the bytes are *not* verified here — the importer re-derives the digest before storing,
    // which is the only check that matters and the one place it belongs.
    let mut function_artifacts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix(FUNCTION_ARTIFACT_PREFIX) else {
            continue;
        };
        let Some(hex) = rest.strip_suffix(".zip") else {
            continue;
        };
        let bytes = budget.read(&mut entry, &name)?;
        function_artifacts.insert(format!("sha256:{hex}"), bytes);
    }

    let contents = PackContents {
        workflows,
        settings,
        pipelines,
        functions,
        function_artifacts,
    };
    validate_namespaced_pack(
        &contents.workflows,
        contents.pipelines.as_ref(),
        &contents.functions,
    )?;
    Ok(contents)
}

/// Reject archive shapes that could consume unbounded CPU, memory, or filesystem work before any
/// entry is decompressed. Read-time limits below remain necessary because the central directory is
/// attacker-controlled too and must not be the only source of truth.
fn validate_archive_layout(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<(), PackError> {
    if archive.len() > MAX_PACK_ENTRIES {
        return Err(format!(
            "pack zip has {} entries, limit is {MAX_PACK_ENTRIES}",
            archive.len()
        )
        .into());
    }

    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        if entry.size() > MAX_PACK_ENTRY_BYTES {
            return Err(format!(
                "pack entry '{name}' expands to {} bytes, per-entry limit is {MAX_PACK_ENTRY_BYTES}",
                entry.size()
            )
            .into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| -> PackError { "pack zip uncompressed size overflows u64".into() })?;
        if total > MAX_PACK_UNCOMPRESSED_BYTES {
            return Err(format!(
                "pack zip expands to more than {MAX_PACK_UNCOMPRESSED_BYTES} bytes"
            )
            .into());
        }
    }
    Ok(())
}

/// The compiled-pack wire contract is intentionally strict: every durable artifact must arrive
/// with its human path as well as the UUID the server will assign or recover. Raw JSON workflow
/// imports are a separate, explicitly acknowledged escape hatch and do not pass through this
/// validator.
pub fn validate_namespaced_pack(
    workflows: &WorkflowBundle,
    pipelines: Option<&PipelineBundle>,
    functions: &[NewFunctionVersion],
) -> Result<(), PackError> {
    let mut paths = BTreeSet::new();
    for workflow in &workflows.workflows {
        let path = required_path(
            "workflow",
            &workflow.name,
            workflow.namespace.as_deref(),
            workflow.key.as_deref(),
        )?;
        if !paths.insert(format!("workflow:{path}")) {
            return Err(format!("pack declares workflow path '{path}' more than once").into());
        }
    }
    if let Some(bundle) = pipelines {
        for pipeline in &bundle.pipelines {
            let path = required_path(
                "pipeline",
                &pipeline.name,
                pipeline.namespace.as_deref(),
                pipeline.key.as_deref(),
            )?;
            if !paths.insert(format!("pipeline:{path}")) {
                return Err(format!("pack declares pipeline path '{path}' more than once").into());
            }
        }
    }
    for function in functions {
        let namespace = required_namespace(
            "function package",
            &function.package.name,
            function.package.namespace.as_deref(),
        )?;
        let path = format!("{namespace}.{}", function.package.name);
        if !paths.insert(format!("function_package:{path}")) {
            return Err(
                format!("pack declares function package path '{path}' more than once").into(),
            );
        }
    }
    Ok(())
}

fn required_path(
    kind: &str,
    display_name: &str,
    namespace: Option<&str>,
    key: Option<&str>,
) -> Result<String, PackError> {
    let namespace = required_namespace(kind, display_name, namespace)?;
    let key = key.filter(|key| !key.trim().is_empty()).ok_or_else(|| {
        format!("{kind} '{display_name}' in a compiled pack must declare a stable key")
    })?;
    Ok(format!("{namespace}.{key}"))
}

fn required_namespace<'a>(
    kind: &str,
    display_name: &str,
    namespace: Option<&'a str>,
) -> Result<&'a str, PackError> {
    let namespace = namespace
        .filter(|namespace| {
            !namespace.trim().is_empty()
                && namespace
                    .split('.')
                    .all(|segment| !segment.trim().is_empty())
        })
        .ok_or_else(|| {
            format!("{kind} '{display_name}' in a compiled pack must declare a dotted namespace")
        })?;
    Ok(namespace)
}

// read and deserialize an optional named entry, returning None when the entry is absent.
fn read_optional_entry<T: serde::de::DeserializeOwned>(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    budget: &mut ReadBudget,
) -> Result<Option<T>, PackError> {
    match archive.by_name(name) {
        Ok(mut file) => {
            let bytes = budget.read(&mut file, name)?;
            Ok(Some(serde_json::from_slice(&bytes)?))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;

mod pack_contents;
pub use pack_contents::PackContents;

mod pack_builder;
pub use pack_builder::PackBuilder;

mod read_budget;
use read_budget::ReadBudget;
