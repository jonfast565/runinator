#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub struct DecompiledRexRap {
    pub source: String,
    pub spans: Vec<RexRapNodeSpan>,
}
