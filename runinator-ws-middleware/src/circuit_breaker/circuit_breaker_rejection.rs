#[allow(unused_imports)]
use super::*;

/// Marks a response synthesized because an API circuit was open. The outer access-metrics layer
/// reads this extension so it never attributes the `503` to overload protection.
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerRejection {
    pub family: CircuitFamily,
}
