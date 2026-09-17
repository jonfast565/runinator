#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct FileQuery {
    pub(super) path: Option<String>,
    pub(super) offset: Option<u64>,
    pub(super) length: Option<usize>,
}
