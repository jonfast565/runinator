#[allow(unused_imports)]
use super::*;

pub struct DeployOptions<'a> {
    pub workspace_root: &'a Path,
    pub manifest_path: &'a Path,
    pub kube_context: Option<&'a str>,
    pub image_map: Option<HashMap<String, String>>,
    pub delete: bool,
    pub command_center_only: bool,
    pub recreate_infra: bool,
    pub expose_direct_ingress: bool,
}
