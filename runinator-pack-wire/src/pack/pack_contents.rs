#[allow(unused_imports)]
use super::*;

pub struct PackContents {
    pub workflows: WorkflowBundle,
    pub settings: Option<SettingsBundle>,
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
