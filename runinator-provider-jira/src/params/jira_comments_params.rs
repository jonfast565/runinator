#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraCommentsParams {
    pub key: String,
    // optional directory to also write downloaded images into (e.g. a worktree the
    // ai step reads from); images are always registered as run artifacts too.
    pub download_dir: Option<String>,
}
