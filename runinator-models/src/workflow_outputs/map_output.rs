#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapOutput {
    pub count: usize,
    pub outputs: Vec<Value>,
}
