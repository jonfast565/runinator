#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct RunParams {
    pub(super) args: Vec<String>,
}
