#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerOutput {
    pub name: String,
    pub circuit_state: String,
    pub tripped: bool,
}
