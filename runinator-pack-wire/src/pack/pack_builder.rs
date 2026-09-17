#[allow(unused_imports)]
use super::*;

/// what goes into a pack zip.
///
/// a builder rather than more positional arguments: the writers already passed three `Option`s in a
/// fixed order, and a fourth and fifth would make every call site a puzzle about which `None` meant
/// what.
#[derive(Default)]
pub struct PackBuilder<'a> {
    pub(super) workflows: Option<&'a WorkflowBundle>,
    pub(super) settings: Option<&'a SettingsBundle>,
    pub(super) pipelines: Option<&'a PipelineBundle>,
    pub(super) functions: Vec<NewFunctionVersion>,
    pub(super) function_artifacts: BTreeMap<String, Vec<u8>>,
}

impl<'a> PackBuilder<'a> {
    pub fn new(workflows: &'a WorkflowBundle) -> Self {
        Self {
            workflows: Some(workflows),
            ..Self::default()
        }
    }

    pub fn settings(mut self, settings: Option<&'a SettingsBundle>) -> Self {
        self.settings = settings;
        self
    }

    /// Compatibility builder spelling used by callers compiled against the legacy wire name.
    pub fn secrets(self, settings: Option<&'a SettingsBundle>) -> Self {
        self.settings(settings)
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
            if let Some(settings) = self.settings {
                zip.start_file(SETTINGS_ENTRY, options)?;
                zip.write_all(&serde_json::to_vec(settings)?)?;
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
                let hex = runinator_hash::hex_part(digest);
                zip.start_file(format!("{FUNCTION_ARTIFACT_PREFIX}{hex}.zip"), options)?;
                zip.write_all(bytes)?;
            }
            zip.finish()?;
        }
        Ok(buffer)
    }
}
