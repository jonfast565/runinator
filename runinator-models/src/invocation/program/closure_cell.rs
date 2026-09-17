#[allow(unused_imports)]
use super::*;

/// one closure: its parameters, its body, and the locals it captured where it was built.
///
/// capture is by value at construction, which is what makes it lexical — the closure sees the
/// bindings visible where it was written, not whatever happens to be in scope where it is applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosureCell {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    pub body: InvocationProgram,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captured: Vec<(String, Value)>,
}
