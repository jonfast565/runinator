#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct GitHubBaseParams {
    pub owner: String,
    pub repo: String,
}
