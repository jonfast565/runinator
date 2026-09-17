#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct ParallelSurface {
    pub labels: Vec<Option<String>>,
    pub selected: Option<Vec<String>>,
    pub stops: Vec<String>,
}
