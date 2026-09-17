#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Whether the source explicitly identified itself with `language rexrap-1`.
    pub language_header: bool,
    /// top-level `fn` definitions, callable from the workflow body, compute blocks, and other
    /// function bodies. siblings of the workflow.
    pub functions: Vec<FunctionDef>,
    /// Pack-local, compile-time-only function modules. Imports select these by path; lowering
    /// embeds their resolved functions and digest into consuming workflows, never as an artifact.
    pub modules: Vec<SourceModule>,
    pub workflows: Vec<Workflow>,
    /// comments after the last top-level item, preserved for lossless formatting.
    pub trailing_comments: Vec<Comment>,
}

impl Document {
    pub fn single_workflow(&self) -> Option<&Workflow> {
        self.workflows.first().filter(|_| self.workflows.len() == 1)
    }

    pub fn single_workflow_mut(&mut self) -> Option<&mut Workflow> {
        if self.workflows.len() == 1 {
            self.workflows.first_mut()
        } else {
            None
        }
    }
}
