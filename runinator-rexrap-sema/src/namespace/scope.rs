#[allow(unused_imports)]
use super::*;

pub(super) struct Scope {
    /// import alias -> target namespace path (e.g. `s` -> `std.strings`).
    pub(super) aliases: HashMap<String, String>,
    /// intrinsic leaves callable bare because their std module was imported unaliased.
    pub(super) bare_intrinsics: HashSet<String>,
    /// user-defined function names (callable bare).
    pub(super) user_fns: HashSet<String>,
    /// typed workflow import alias -> durable-path selector. This remains source-only until the
    /// pack importer resolves it to an ArtifactRef UUID/digest.
    pub(super) workflow_aliases: HashMap<String, WorkflowImport>,
    /// typed function-package import alias -> the package's authoring path. An action written as
    /// `pdf.render(...)` becomes `functions.acme.shared.pdf.render(...)` before lowering, where
    /// the normal function catalog resolver records the exact package/version/export binding.
    pub(super) function_aliases: HashMap<String, String>,
    /// typed settings import alias -> durable authoring namespace. `shared.timeout` is config by
    /// default; `shared.secret.token` selects the late-resolved secret family explicitly.
    pub(super) settings_aliases: HashMap<String, String>,
    /// source-module import alias -> exported leaf -> deterministic embedded function name.
    pub(super) module_aliases: HashMap<String, HashMap<String, String>>,
    /// bare calls rewritten while preparing one module's private function namespace.
    pub(super) function_renames: HashMap<String, String>,
}

impl Scope {
    /// an empty scope for standalone fragments: no imports, no user functions.
    pub(super) fn empty() -> Self {
        Self {
            aliases: HashMap::new(),
            bare_intrinsics: HashSet::new(),
            user_fns: HashSet::new(),
            workflow_aliases: HashMap::new(),
            function_aliases: HashMap::new(),
            settings_aliases: HashMap::new(),
            module_aliases: HashMap::new(),
            function_renames: HashMap::new(),
        }
    }
}
