#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskResponseSchema {
    pub success: bool,
    pub message: String,
}
