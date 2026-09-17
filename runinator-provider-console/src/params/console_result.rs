#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(crate) struct ConsoleResult {
    pub success: bool,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub command: String,
}
