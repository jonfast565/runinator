#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct GitHubBaseParams {
    pub token: String,
    pub owner: String,
    pub repo: String,
}
