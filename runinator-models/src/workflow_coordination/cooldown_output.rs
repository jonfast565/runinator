#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownOutput {
    pub name: String,
    pub skipped: bool,
    pub remaining_seconds: i64,
}
