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
pub trait NodeKindSpec: Send + Sync {
    /// the kind this spec describes.
    fn kind(&self) -> WorkflowNodeKind;

    /// ui/authoring metadata: palette entry, field schema, edge slots, default template.
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

/// how the graph walkers treat a node kind.
///
/// each field replaced a free-standing `matches!` list in a different file. a new kind picks its
/// role once here rather than being silently omitted from four separate lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRole {
    /// may be entered as a branch or body target — true for everything but `start`/`end`/`fail`.
    pub runnable_entry: bool,
    /// an entry point: the runtime places a cursor here, and no edge may target it.
    ///
    /// distinct from `runnable_entry`, which the two flags used to conflate because `start` was the
    /// only entry point and it is not a legal region entry. `interrupt` is both — a handler region
    /// legitimately starts there — so "a cursor may sit here" and "an edge may point here" had to
    /// come apart. [`TargetRule`] reads this one.
    pub entry_point: bool,
    /// settles the run when reached.
    pub terminal: bool,
    /// records an output addressable downstream as `steps.<id>.output`.
    pub produces_output: bool,
    /// re-entered by design, so a back edge to it is a loop rather than a cycle error.
    pub reentrant: bool,
    /// modelled by the dry-run simulator; the rest need fan-out bookkeeping the walk lacks.
    pub simulatable: bool,
    /// may appear inside an interrupt handler region.
    ///
    /// this is an opt-in allowlist, defaulting to `false` on every role: a kind is unsupported in a
    /// handler until someone deliberately supports it. that is what keeps the blast radius of the
    /// feature small — a handler is a bounded side-channel, so it may not park (which would pin the
    /// suspended thread open), fan out (whose cursors have no handler to belong to), or run away.
    pub handler_safe: bool,
    /// a cursor sitting on this kind may be interrupted.
    ///
    /// false where a cursor is not a thread doing work: the graph endpoints, and `join`, where the
    /// cursor represents coordination state rather than a position to come back to.
    pub interruptible: bool,
}

impl GraphRole {
    /// the ordinary case: a runnable step that records an output and the simulator models.
    pub const STEP: Self = Self {
        runnable_entry: true,
        entry_point: false,
        terminal: false,
        produces_output: true,
        reentrant: false,
        simulatable: true,
        handler_safe: false,
        interruptible: true,
    };

    /// `start`: entered only as the run's entry point, and produces nothing addressable.
    pub const START: Self = Self {
        runnable_entry: false,
        entry_point: true,
        terminal: false,
        produces_output: false,
        reentrant: false,
        simulatable: true,
        handler_safe: false,
        interruptible: false,
    };

    /// `end`/`fail`: settles the run, and produces nothing addressable.
    pub const TERMINAL: Self = Self {
        runnable_entry: false,
        entry_point: false,
        terminal: true,
        produces_output: false,
        reentrant: false,
        simulatable: true,
        handler_safe: false,
        interruptible: false,
    };

    /// a step whose output is not addressable downstream.
    pub const fn without_output(self) -> Self {
        Self {
            produces_output: false,
            ..self
        }
    }

    /// a step the dry-run simulator does not model.
    pub const fn not_simulatable(self) -> Self {
        Self {
            simulatable: false,
            ..self
        }
    }

    /// a step a back edge may legitimately return to.
    pub const fn reentrant(self) -> Self {
        Self {
            reentrant: true,
            ..self
        }
    }

    /// a step an interrupt handler region may contain. opt in only for kinds that cannot park, fan
    /// out, or run unbounded — a handler must finish and hand control back.
    pub const fn handler_safe(self) -> Self {
        Self {
            handler_safe: true,
            ..self
        }
    }

    /// a step a cursor may not be interrupted while sitting on.
    pub const fn not_interruptible(self) -> Self {
        Self {
            interruptible: false,
            ..self
        }
    }
}

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

/// one node target read out of a node's parameters.
#[derive(Debug, Clone)]
pub struct TargetSlot {
    /// the catalog edge-slot key this target came from; a conformance test pins the two together.
    pub key: &'static str,
    /// how the target is described in a validation error ("switch case target").
    pub label: &'static str,
    /// what the target is allowed to be.
    pub rule: TargetRule,
    /// the referenced node.
    pub target: WorkflowNodeRef,
}

impl TargetSlot {
    /// a routing target: anything that is not an entry point.
    pub fn non_entry(key: &'static str, label: &'static str, target: WorkflowNodeRef) -> Self {
        Self {
            key,
            label,
            rule: TargetRule::NonEntry,
            target,
        }
    }

    /// the entry of a body or branch region: a runnable, non-terminal node.
    pub fn runnable(key: &'static str, label: &'static str, target: WorkflowNodeRef) -> Self {
        Self {
            key,
            label,
            rule: TargetRule::RunnableEntry,
            target,
        }
    }
}

/// provider actions indexed by `(provider, function)`.
pub struct ActionCatalog<'a> {
    actions: HashMap<(&'a str, &'a str), &'a ActionMetadata>,
}

impl<'a> ActionCatalog<'a> {
    /// index every action the given providers expose.
    pub fn new(providers: &'a [ProviderMetadata]) -> Self {
        let actions = providers
            .iter()
            .flat_map(|provider| {
                provider.actions.iter().map(move |action| {
                    (
                        (provider.name.as_str(), action.function_name.as_str()),
                        action,
                    )
                })
            })
            .collect();
        Self { actions }
    }

    /// the action a `provider.function` pair names, if it is registered.
    pub fn get(&self, provider: &str, function: &str) -> Option<&'a ActionMetadata> {
        self.actions.get(&(provider, function)).copied()
    }
}

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
