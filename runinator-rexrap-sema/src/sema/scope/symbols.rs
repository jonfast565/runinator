#[allow(unused_imports)]
use super::*;

pub(crate) struct Symbols {
    pub labels: HashSet<String>,
    pub registry: crate::registry::FunctionRegistry,
}
