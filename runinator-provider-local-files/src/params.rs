use serde::Deserialize;

use crate::errors::INVALID_PARAMS;

runinator_provider_support::provider_parse_params!(INVALID_PARAMS);

/// the single-path parameter shared by read_file/list_dir/stat/delete.
#[derive(Deserialize)]
pub(crate) struct PathParams {
    pub path: String,
}

/// write_file parameters.
#[derive(Deserialize)]
pub(crate) struct WriteParams {
    pub path: String,
    pub content: String,
}
