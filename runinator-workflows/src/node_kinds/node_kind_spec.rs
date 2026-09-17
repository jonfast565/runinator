#[allow(unused_imports)]
use super::*;

pub trait NodeKindSpec: Send + Sync {
    /// the kind this spec describes.
    fn kind(&self) -> WorkflowNodeKind;

    /// UI/authoring metadata: palette entry, field schema, edge slots, default template.
    fn metadata(&self) -> WorkflowNodeKindMetadata;

    /// how the graph walkers treat this kind.
    fn graph_role(&self) -> GraphRole;

    /// the node targets carried in this node's parameters.
    ///
    /// this is the single source for graph edges, reference validation, and the catalog's control
    /// edge slots — those three used to encode the same fact separately.
    fn target_slots(
        &self,
        _node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(Vec::new())
    }

    /// check that this node's own parameters parse and are well-formed, independent of the graph.
    fn check_parameters(&self, _node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        Ok(())
    }

    /// the node's output type when it is known before the run, keyed on `steps.<id>.output`.
    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<RuninatorType>, WorkflowValidationError> {
        Ok(None)
    }
}
