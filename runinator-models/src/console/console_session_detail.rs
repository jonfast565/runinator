#[allow(unused_imports)]
use super::*;

/// a session with everything under it, as the API and UI read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleSessionDetail {
    #[serde(flatten)]
    pub session: ConsoleSession,
    #[serde(default)]
    pub cells: Vec<ConsoleCell>,
    #[serde(default)]
    pub bindings: Vec<ConsoleBinding>,
    #[serde(default)]
    pub functions: Vec<ConsoleFunction>,
}
