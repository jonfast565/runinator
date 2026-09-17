#[allow(unused_imports)]
use super::*;

/// what charging the budget cost, and whether that was the last one it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charge {
    /// 1-based count of consecutive failures, including this one.
    pub attempt: u32,
    /// true when the budget is now spent and the agent should stop.
    pub spent: bool,
}
