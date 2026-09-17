#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct PrepareCheckoutParams {
    pub repository: String,
    pub revision: String,
}
