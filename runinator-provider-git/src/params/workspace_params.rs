#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct WorkspaceParams {
    pub workspace: Option<String>,
    pub repo: Option<String>,
}
