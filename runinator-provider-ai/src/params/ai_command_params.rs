#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct AiCommandParams {
    pub command: String,
    pub input: Option<Value>,
}
