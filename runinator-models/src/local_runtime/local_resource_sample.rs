#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalResourceSample {
    pub sampled_at: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
}
