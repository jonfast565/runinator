//! the per-kind descriptor registry.
//!
//! every fact the workflow layer knows about a node kind lives on one [`NodeKindSpec`]: its
//! authoring metadata, its role in the graph, the node targets its parameters carry, the shape
//! check its parameters must pass, and its statically-known output type. `catalog.rs`,
//! `parameters.rs`, `validation.rs`, `typing.rs`, and `simulate.rs` read those facts from here
//! instead of each keeping a parallel `match` over the enum.
//!
//! adding a node kind is a new spec plus one arm in [`spec_for`], which is exhaustive — so the
//! compiler, not review, is what notices the omission.
//!
//! two per-kind concerns deliberately stay outside this registry: `typing.rs`'s per-kind type
//! checks need the private inference context, and `simulate.rs`'s per-kind evaluation needs the
//! simulator's private outcome type and its `&mut dyn SimulationEnv`. both are single-sited and
//! exhaustively matched, so they cannot silently disagree with anything; they read the *facts*
//! they used to re-derive ([`GraphRole`], [`NodeKindSpec::output_type`]) from here.

use std::collections::HashMap;

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::providers::{ActionMetadata, ProviderMetadata};
use runinator_models::types::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind, WorkflowNodeRef};

use runinator_compute::WorkflowValidationError;

mod builders;
mod concurrency;
mod control_flow;
mod io;
mod sync;
mod task;
mod terminal;

#[cfg(test)]
mod tests;

/// everything the workflow layer knows about one node kind.

/// what a node reference is allowed to point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRule {
    /// any node that is not an entry point — the rule an ordinary transition obeys.
    NonEntry,
    /// a runnable, non-terminal node: the entry of a body or branch region.
    RunnableEntry,
    /// a node that records an output, so `$ref` can read it.
    OutputProducing,
}

impl TargetRule {
    /// whether a target of this kind satisfies the rule.
    pub fn accepts(self, kind: &WorkflowNodeKind) -> bool {
        let role = spec_for(kind).graph_role();
        match self {
            Self::NonEntry => !role.entry_point,
            // an entry point is where the *runtime* places a cursor. `interrupt` is a legal region
            // entry and so is `runnable_entry`, but a body or branch may still not route into one.
            Self::RunnableEntry => role.runnable_entry && !role.entry_point,
            Self::OutputProducing => role.produces_output,
        }
    }

    /// the phrase used in the validation error when a target fails the rule.
    pub fn expected(self) -> &'static str {
        match self {
            Self::NonEntry => "a node that is not an entry point",
            Self::RunnableEntry => "a runnable, non-terminal node",
            Self::OutputProducing => "an output-producing node",
        }
    }
}

/// provider actions indexed by `(provider, function)`.

/// the descriptor for a node kind. exhaustive: a new variant fails to compile until it is listed.
pub fn spec_for(kind: &WorkflowNodeKind) -> &'static dyn NodeKindSpec {
    match kind {
        WorkflowNodeKind::Start => &terminal::Start,
        WorkflowNodeKind::End => &terminal::End,
        WorkflowNodeKind::Fail => &terminal::Fail,
        WorkflowNodeKind::Resume => &terminal::Resume,
        WorkflowNodeKind::Interrupt => &terminal::Interrupt,
        WorkflowNodeKind::Action => &task::Action,
        WorkflowNodeKind::Invocation => &task::Invocation,
        WorkflowNodeKind::Subflow => &task::Subflow,
        WorkflowNodeKind::Wait => &control_flow::Wait,
        WorkflowNodeKind::Condition => &control_flow::Condition,
        WorkflowNodeKind::Switch => &control_flow::Switch,
        WorkflowNodeKind::Toggle => &control_flow::Toggle,
        WorkflowNodeKind::Percentage => &control_flow::Percentage,
        WorkflowNodeKind::Approval => &control_flow::Approval,
        WorkflowNodeKind::Gate => &control_flow::Gate,
        WorkflowNodeKind::Signal => &control_flow::Signal,
        WorkflowNodeKind::Loop => &control_flow::Loop,
        WorkflowNodeKind::Try => &control_flow::Try,
        WorkflowNodeKind::Assert => &control_flow::Assert,
        WorkflowNodeKind::Checkpoint => &control_flow::Checkpoint,
        WorkflowNodeKind::Parallel => &concurrency::Parallel,
        WorkflowNodeKind::Join => &concurrency::Join,
        WorkflowNodeKind::Map => &concurrency::Map,
        WorkflowNodeKind::Race => &concurrency::Race,
        WorkflowNodeKind::Output => &io::Output,
        WorkflowNodeKind::Input => &io::Input,
        WorkflowNodeKind::Config => &io::Config,
        WorkflowNodeKind::Transform => &io::Transform,
        WorkflowNodeKind::Audit => &io::Audit,
        WorkflowNodeKind::EventSource => &io::EventSource,
        WorkflowNodeKind::Mutex => &sync::Mutex,
        WorkflowNodeKind::Throttle => &sync::Throttle,
        WorkflowNodeKind::Cooldown => &sync::Cooldown,
        WorkflowNodeKind::AwaitRun => &sync::AwaitRun,
        WorkflowNodeKind::Debounce => &sync::Debounce,
        WorkflowNodeKind::Collect => &sync::Collect,
        WorkflowNodeKind::Barrier => &sync::Barrier,
        WorkflowNodeKind::CircuitBreaker => &sync::CircuitBreaker,
    }
}

/// the graph role of a node kind.
pub fn graph_role(kind: &WorkflowNodeKind) -> GraphRole {
    spec_for(kind).graph_role()
}

/// the node targets carried in a node's parameters.
pub fn target_slots(node: &WorkflowNode) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
    spec_for(&node.kind).target_slots(node)
}

mod node_kind_spec;
pub use node_kind_spec::NodeKindSpec;

mod graph_role;
pub use graph_role::GraphRole;

mod target_slot;
pub use target_slot::TargetSlot;

mod action_catalog;
pub use action_catalog::ActionCatalog;
