#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskResponse {
    pub success: bool,
    pub message: String,
}
