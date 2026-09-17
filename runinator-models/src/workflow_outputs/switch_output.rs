#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchOutput {
    pub target: Option<String>,
}
