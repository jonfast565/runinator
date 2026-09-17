#[allow(unused_imports)]
use super::*;

/// one named function body plus the parameters it binds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationFunction {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    pub body: InvocationProgram,
    /// the annotated recursion cap from `@recursive(max_depth: N)`, when the author set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}
