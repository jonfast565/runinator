#[allow(unused_imports)]
use super::*;

/// a compiled unit: the entry program plus every function it can call by name.
///
/// user functions live here rather than in `metadata.functions` because they are *code*, and the
/// point of the ir is that there is one representation of code. the module is what a continuation
/// is resumed against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationModule {
    /// the ir version this module was compiled at; see [`INVOCATION_IR_VERSION`].
    pub version: u32,
    /// the program that runs when the invocation starts.
    pub entry: InvocationProgram,
    /// callable-by-name function bodies, keyed by the name the program calls them under.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<InvocationFunction>,
}

impl InvocationModule {
    /// a module holding a single program and no functions.
    pub fn new(entry: InvocationProgram) -> Self {
        Self {
            version: INVOCATION_IR_VERSION,
            entry,
            functions: Vec::new(),
        }
    }

    /// look up a function body by name.
    pub fn function(&self, name: &str) -> Option<&InvocationFunction> {
        self.functions.iter().find(|item| item.name == name)
    }

    /// whether this module's version is one the current vm understands.
    pub fn is_supported(&self) -> bool {
        self.version == INVOCATION_IR_VERSION
    }
}
