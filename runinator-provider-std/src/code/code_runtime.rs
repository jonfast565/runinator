#[allow(unused_imports)]
use super::*;

pub(super) struct CodeRuntime {
    pub(super) image: String,
    pub(super) setup_script: String,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) executable: Option<String>,
    pub(super) build_args: Vec<String>,
    pub(super) run_args: Vec<String>,
    pub(super) limits: RuntimeLimits,
}
