#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct ArchivePatchParams {
    pub workspace: Option<String>,
    pub name: Option<String>,
}
