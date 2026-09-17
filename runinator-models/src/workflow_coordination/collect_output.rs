#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectOutput {
    pub items: Vec<Value>,
    pub count: usize,
    pub reason: String,
}
