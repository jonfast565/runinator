use serde::Deserialize;

use crate::errors::INVALID_PARAMS;

runinator_provider_support::provider_parse_params!(INVALID_PARAMS);

mod path_params;
pub(crate) use path_params::PathParams;

mod write_params;
pub(crate) use write_params::WriteParams;
