#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct PreparedCheckoutResult {
    pub(super) repository: String,
    pub(super) revision: String,
    pub(super) sha: String,
    pub(super) workspace: String,
}
