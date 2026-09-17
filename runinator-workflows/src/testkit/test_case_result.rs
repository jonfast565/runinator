#[allow(unused_imports)]
use super::*;

/// the outcome of running one case.
#[derive(Debug, Clone)]
pub struct TestCaseResult {
    pub name: String,
    pub passed: bool,
    /// human-readable assertion failures; empty when the case passed.
    pub failures: Vec<String>,
    pub run: SimulationRun,
}
