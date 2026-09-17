#[allow(unused_imports)]
use super::*;

/// write_file parameters.
#[derive(Deserialize)]
pub(crate) struct WriteParams {
    pub path: String,
    pub content: String,
}
