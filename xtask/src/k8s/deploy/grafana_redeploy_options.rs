#[allow(unused_imports)]
use super::*;

pub struct GrafanaRedeployOptions<'a> {
    pub workspace_root: &'a Path,
    pub manifest_path: &'a Path,
    pub kube_context: Option<&'a str>,
}
