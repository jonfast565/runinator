#[allow(unused_imports)]
use super::*;

/// One active top-level REXRAP definition in a console session.
///
/// Source is stored per declaration rather than reconstructing it from a notebook cell: the active
/// library has latest-successful semantics, and a cell may define several names independently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleFunction {
    pub id: Uuid,
    pub session_id: Uuid,
    /// The cell whose successful execution most recently published this definition.
    pub cell_id: Uuid,
    pub name: String,
    pub is_task: bool,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
