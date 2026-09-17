#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleOutput {
    pub name: String,
    pub admitted: bool,
}
