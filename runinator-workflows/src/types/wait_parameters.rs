#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct WaitParameters {
    pub seconds: i64,
    pub until_status: Option<String>,
    pub initial_status: String,
}
