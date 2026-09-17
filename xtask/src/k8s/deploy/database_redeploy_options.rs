#[allow(unused_imports)]
use super::*;

pub struct DatabaseRedeployOptions<'a> {
    pub workspace_root: &'a Path,
    pub manifest_path: &'a Path,
    pub kube_context: Option<&'a str>,
    pub from_scratch: bool,
}
