// maps a node kind to the handler that serves it.
//
// replaces the former `match node.kind` dispatch: adding a node type is registering one handler
// here. lookup is a linear scan over the ~17 builtins, which is negligible per scheduling tick.

use std::sync::Arc;

use runinator_models::workflows::WorkflowNodeKind;

use crate::nodes::handler::NodeHandler;
use crate::nodes::handlers;

pub struct NodeRegistry {
    handlers: Vec<Arc<dyn NodeHandler>>,
}

impl NodeRegistry {
    /// build the registry with every builtin node handler registered.
    pub fn with_builtins() -> Self {
        Self {
            handlers: handlers::builtins(),
        }
    }

    /// the handler for `kind`, or none if unregistered.
    pub fn get(&self, kind: &WorkflowNodeKind) -> Option<&Arc<dyn NodeHandler>> {
        self.handlers.iter().find(|handler| &handler.kind() == kind)
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}
