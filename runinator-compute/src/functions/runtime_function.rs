#[allow(unused_imports)]
use super::*;

pub struct RuntimeFunction {
    pub params: Vec<String>,
    pub body: FunctionBody,
    pub max_depth: Option<u32>,
}
