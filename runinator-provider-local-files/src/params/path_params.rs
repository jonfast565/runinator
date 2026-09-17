#[allow(unused_imports)]
use super::*;

/// the single-path parameter shared by read_file/list_dir/stat/delete.
#[derive(Deserialize)]
pub(crate) struct PathParams {
    pub path: String,
}
