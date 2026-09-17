#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutexOutput {
    pub name: String,
    pub acquired: bool,
    #[serde(default)]
    pub released: bool,
}
