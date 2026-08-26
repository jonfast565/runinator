// shared layout for the compiled workflow pack uploaded to the web service. the client compiles a
// pack (`.rexrap`/`.rexraps`/`.rexrapm`) and zips the resulting json artifacts; the web service unzips and
// imports them. compilation stays on the client — the backend only reads the compiled json here.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Write};

use runinator_models::bundles::SecretBundle;
use runinator_models::functions::NewFunctionVersion;
use runinator_models::pipelines::PipelineBundle;
use runinator_models::workflows::WorkflowBundle;
use zip::write::SimpleFileOptions;

/// zip entry holding the compiled `WorkflowBundle` json (always present).
pub const WORKFLOWS_ENTRY: &str = "workflows.json";
/// zip entry holding the compiled `SecretBundle` json (optional).
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

/// error type for pack zip read/write; boxes zip and serde failures alike.
pub type PackError = Box<dyn std::error::Error + Send + Sync>;

/// what a pack zip carries once read back.
pub struct PackContents {
    pub workflows: WorkflowBundle,
    pub secrets: Option<SecretBundle>,
    pub pipelines: Option<PipelineBundle>,
    /// packaged-function publish requests, imported before workflows so a workflow that binds to
    /// one can be validated against it.
    pub functions: Vec<NewFunctionVersion>,
    /// function archives carried in the pack, keyed by `sha256:<hex>`.
    ///
    /// only the ones the server said it was missing: a pack that shipped every artifact every time
    /// would push megabytes over a 10 MB request limit to re-send bytes the server already holds.
    pub function_artifacts: BTreeMap<String, Vec<u8>>,
}

/// what goes into a pack zip.
///
/// a builder rather than more positional arguments: the writers already passed three `Option`s in a
/// fixed order, and a fourth and fifth would make every call site a puzzle about which `None` meant
/// what.
#[derive(Default)]
pub struct PackBuilder<'a> {
    workflows: Option<&'a WorkflowBundle>,
    secrets: Option<&'a SecretBundle>,
    pipelines: Option<&'a PipelineBundle>,
    functions: Vec<NewFunctionVersion>,
    function_artifacts: BTreeMap<String, Vec<u8>>,
}

impl<'a> PackBuilder<'a> {
    pub fn new(workflows: &'a WorkflowBundle) -> Self {
        Self {
            workflows: Some(workflows),
            ..Self::default()
        }
    }

    pub fn secrets(mut self, secrets: Option<&'a SecretBundle>) -> Self {
        self.secrets = secrets;
        self
    }

    pub fn pipelines(mut self, pipelines: Option<&'a PipelineBundle>) -> Self {
        self.pipelines = pipelines;
        self
    }

    pub fn functions(mut self, functions: Vec<NewFunctionVersion>) -> Self {
        self.functions = functions;
        self
    }

    /// carry one function archive, keyed by its digest.
    pub fn function_artifact(mut self, digest: impl Into<String>, bytes: Vec<u8>) -> Self {
        self.function_artifacts.insert(digest.into(), bytes);
        self
    }

    pub fn build(self) -> Result<Vec<u8>, PackError> {
        let workflows = self
            .workflows
            .ok_or_else(|| -> PackError { "pack must carry a workflow bundle".into() })?;
        validate_namespaced_pack(workflows, self.pipelines, &self.functions)?;
        let mut buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
            // stored (uncompressed) keeps the zip backend dependency-free. note function archives
            // are already deflate-free zips of their own, so nesting them costs nothing here.
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file(WORKFLOWS_ENTRY, options)?;
            zip.write_all(&serde_json::to_vec(workflows)?)?;
            if let Some(secrets) = self.secrets {
                zip.start_file(SECRETS_ENTRY, options)?;
                zip.write_all(&serde_json::to_vec(secrets)?)?;
            }
            if let Some(pipelines) = self.pipelines.filter(|p| !p.pipelines.is_empty()) {
                zip.start_file(PIPELINES_ENTRY, options)?;
                zip.write_all(&serde_json::to_vec(pipelines)?)?;
            }
            if !self.functions.is_empty() {
                zip.start_file(FUNCTIONS_ENTRY, options)?;
                zip.write_all(&serde_json::to_vec(&self.functions)?)?;
            }
            for (digest, bytes) in &self.function_artifacts {
                let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
                zip.start_file(format!("{FUNCTION_ARTIFACT_PREFIX}{hex}.zip"), options)?;
                zip.write_all(bytes)?;
            }
            zip.finish()?;
        }
        Ok(buffer)
    }
}

/// build a compiled pack zip from a workflow bundle and optional secret / pipeline bundles.
///
/// kept for the callers that carry nothing else; anything richer goes through [`PackBuilder`].
pub fn build_pack_zip(
    workflows: &WorkflowBundle,
    secrets: Option<&SecretBundle>,
    pipelines: Option<&PipelineBundle>,
) -> Result<Vec<u8>, PackError> {
    PackBuilder::new(workflows)
        .secrets(secrets)
        .pipelines(pipelines)
        .build()
}

/// read a compiled pack zip back into its workflow bundle and optional secret / pipeline bundles.
pub fn read_pack_zip(bytes: &[u8]) -> Result<PackContents, PackError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let workflows: WorkflowBundle = {
        let mut file = archive
            .by_name(WORKFLOWS_ENTRY)
            .map_err(|_| format!("pack zip missing '{WORKFLOWS_ENTRY}'"))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        serde_json::from_str(&text)?
    };
    let secrets = read_optional_entry(&mut archive, SECRETS_ENTRY)?;
    let pipelines = read_optional_entry(&mut archive, PIPELINES_ENTRY)?;
    let functions = read_optional_entry(&mut archive, FUNCTIONS_ENTRY)?.unwrap_or_default();

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
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        function_artifacts.insert(format!("sha256:{hex}"), bytes);
    }

    let contents = PackContents {
        workflows,
        secrets,
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
) -> Result<Option<T>, PackError> {
    match archive.by_name(name) {
        Ok(mut file) => {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            Ok(Some(serde_json::from_str(&text)?))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
