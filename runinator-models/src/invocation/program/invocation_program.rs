#[allow(unused_imports)]
use super::*;

/// a program is a flat instruction sequence over an operand stack.
///
/// flat rather than a tree because a continuation has to name a *resume point*, and an index into a
/// vector is a stable name that survives serialization. a tree would need a path, and every edit to
/// the shape of the tree would change what that path meant.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InvocationProgram {
    pub instructions: Vec<InvocationInstruction>,
}

impl InvocationProgram {
    pub fn new(instructions: Vec<InvocationInstruction>) -> Self {
        Self { instructions }
    }

    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    pub fn get(&self, ip: usize) -> Option<&InvocationInstruction> {
        self.instructions.get(ip)
    }
}
