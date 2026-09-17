#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct WorkflowQuery {
    pub name: Option<String>,
}
