use std::collections::{HashMap, HashSet};

use runinator_models::{
    interrupt::InterruptDeclaration,
    providers::ProviderMetadata,
    types::RuninatorType,
    value::Value,
    workflows::{
        WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowTransitions,
    },
};

use crate::node_kinds::{TargetRule, graph_role, spec_for, target_slots};
use crate::parameters::{parse_map_parameters, value_refs};
use crate::refs::expand_workflow_refs;
use crate::typing::validate_workflow_types;
use runinator_compute::WorkflowValidationError;
use runinator_compute::validate_condition;
use runinator_models::workflow_ast::WorkflowRefSource;

pub fn parse_nodes(
    workflow: &WorkflowDefinition,
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let definition = expand_workflow_refs(workflow)?;
    let start = definition
        .start
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(WorkflowValidationError::MissingStart)?
        .to_string();
    let nodes = definition.nodes;
    if nodes.is_empty() {
        return Err(WorkflowValidationError::MissingNodes);
    }
    Ok((start, nodes))
}

pub fn validate_workflow(
    workflow: &WorkflowDefinition,
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let (start, nodes) = parse_nodes(workflow)?;
    let mut seen = HashSet::new();
    let ids = nodes
        .iter()
        .map(|node| {
            if !seen.insert(node.id.as_str().to_string()) {
                return Err(WorkflowValidationError::DuplicateNode(
                    node.id.as_str().to_string(),
                ));
            }
            Ok(node.id.as_str().to_string())
        })
        .collect::<Result<HashSet<_>, _>>()?;

    if !ids.contains(&start) {
        return Err(WorkflowValidationError::MissingStartNode(start));
    }
    if nodes
        .iter()
        .find(|node| node.id.as_str() == start)
        .is_none_or(|node| node.kind != WorkflowNodeKind::Start)
    {
        return Err(WorkflowValidationError::MissingStartKind);
    }
    if !nodes.iter().any(|node| node.kind == WorkflowNodeKind::End) {
        return Err(WorkflowValidationError::MissingEndNode);
    }

    let node_map: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    for node in &nodes {
        // the kind's own shape check: a missing action, an unnamed subflow, malformed control
        // parameters. it runs first so "this node does not parse" beats "its retry count is odd".
        spec_for(&node.kind).check_parameters(node)?;
        if node.retry.max_attempts <= 0 {
            return Err(WorkflowValidationError::InvalidRetry(
                node.id.as_str().to_string(),
            ));
        }
        if node.timeout_seconds.is_some_and(|timeout| timeout <= 0) {
            return Err(WorkflowValidationError::InvalidTimeout(
                node.id.as_str().to_string(),
            ));
        }
        if node.max_iterations.is_some_and(|limit| limit <= 0) {
            return Err(WorkflowValidationError::InvalidLoopLimit(
                node.id.as_str().to_string(),
            ));
        }
        if node.reentry.enabled && node.reentry.max_visits <= 0 {
            return Err(WorkflowValidationError::InvalidReentry(
                node.id.as_str().to_string(),
            ));
        }
        validate_condition(&node.condition.to_value())?;
        for target in transition_targets(&node.transitions) {
            validate_node_ref(node, &target, "transition", TargetRule::NonEntry, &node_map)?;
        }
        for slot in target_slots(node)? {
            validate_node_ref(node, &slot.target, slot.label, slot.rule, &node_map)?;
        }
        for reference in value_refs(node)? {
            if let WorkflowRefSource::NodeOutput(target) = reference.source {
                validate_node_ref(
                    node,
                    &target,
                    "node output reference",
                    TargetRule::OutputProducing,
                    &node_map,
                )?;
            }
        }
        if let Some(target) = node.reentry.on_exhausted.as_ref() {
            validate_node_ref(
                node,
                target,
                "reentry on_exhausted",
                TargetRule::NonEntry,
                &node_map,
            )?;
        }
    }

    validate_graph_cycles(&start, &nodes)?;
    validate_map_concurrency_bodies(&nodes)?;
    validate_mutex_sections(&start, &nodes)?;
    validate_interrupt_handlers(workflow, &start, &nodes)?;

    Ok((start, nodes))
}

fn validate_node_ref(
    node: &WorkflowNode,
    target: &WorkflowNodeRef,
    label: &str,
    rule: TargetRule,
    node_map: &HashMap<&str, &WorkflowNode>,
) -> Result<(), WorkflowValidationError> {
    let Some(target_node) = node_map.get(target.as_str()) else {
        return Err(WorkflowValidationError::MissingTransition {
            node: node.id.as_str().to_string(),
            target: target.as_str().to_string(),
        });
    };
    if rule.accepts(&target_node.kind) {
        return Ok(());
    }
    Err(WorkflowValidationError::InvalidNodeReferenceType {
        node: node.id.as_str().to_string(),
        reference: label.to_string(),
        target: target.as_str().to_string(),
        target_kind: format!("{:?}", target_node.kind),
        expected: rule.expected().to_string(),
    })
}

/// a concurrent `map` body runs as an isolated child run, so for `concurrency > 1` the body must be a
/// single-entry/single-exit region: reachable only through the map `target`, exiting only back to the
/// map node, free of terminal `start`/`end`/`fail` nodes, and not read by `$ref` from outside.
fn validate_map_concurrency_bodies(nodes: &[WorkflowNode]) -> Result<(), WorkflowValidationError> {
    let node_map: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    for node in nodes {
        if node.kind != WorkflowNodeKind::Map {
            continue;
        }
        let params = parse_map_parameters(node)?;
        if params.concurrency.unwrap_or(1) <= 1 {
            continue;
        }
        let map_id = node.id.as_str();
        let region = collect_body_region(params.target.as_str(), map_id, &node_map)?;
        let not_isolatable =
            |reason: String| WorkflowValidationError::MapConcurrencyBodyNotIsolatable {
                node: map_id.to_string(),
                reason,
            };

        // region nodes must not be terminal kinds and must exit only back to the map node.
        for region_id in &region {
            let region_node = node_map
                .get(region_id.as_str())
                .ok_or_else(|| not_isolatable(format!("body node '{region_id}' does not exist")))?;
            let role = graph_role(&region_node.kind);
            if !role.runnable_entry || role.entry_point {
                return Err(not_isolatable(format!(
                    "body node '{region_id}' is a {:?} node",
                    region_node.kind
                )));
            }
            for target in body_edges(region_node)? {
                let target = target.as_str();
                if target != map_id && !region.contains(target) {
                    return Err(not_isolatable(format!(
                        "body node '{region_id}' exits to '{target}' outside the map body"
                    )));
                }
            }
        }

        // nothing outside the body may enter it or read its outputs (cross-item escape).
        for other in nodes {
            let other_id = other.id.as_str();
            if other_id == map_id || region.contains(other_id) {
                continue;
            }
            for target in body_edges(other)? {
                if region.contains(target.as_str()) {
                    return Err(not_isolatable(format!(
                        "node '{other_id}' enters the map body at '{}'",
                        target.as_str()
                    )));
                }
            }
            for reference in value_refs(other)? {
                if let WorkflowRefSource::NodeOutput(target) = reference.source
                    && region.contains(target.as_str())
                {
                    return Err(not_isolatable(format!(
                        "node '{other_id}' reads body output of '{}'",
                        target.as_str()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// the nodes making up the interrupt handler region entered at `entry`.
///
/// shared with the reducer, which re-checks the region at runtime rather than trusting that the
/// definition it is executing was validated by a binary with this same allowlist. sharing the walk
/// is the point: a region the validator accepted and the runtime rejected would be a silent stall.
pub fn interrupt_region(
    entry: &str,
    nodes: &[WorkflowNode],
) -> Result<HashSet<String>, WorkflowValidationError> {
    let node_map: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    // "\0" is an id no node can carry, so the walk stops only where the graph does.
    collect_body_region(entry, "\0", &node_map)
}

/// every node belonging to any declared interrupt handler region.
///
/// this is how "was this node run produced by a handler?" is answered without asking a cursor. a
/// handler cursor is ephemeral — it retires the moment the region ends — but region membership is a
/// static property of the graph, so it stays true for as long as the run's history does.
pub fn interrupt_region_nodes(
    workflow: &WorkflowDefinition,
    nodes: &[WorkflowNode],
) -> HashSet<String> {
    let mut all = HashSet::new();
    for declaration in interrupt_declarations(workflow, nodes) {
        if let Ok(region) = interrupt_region(&declaration.handler, nodes) {
            all.extend(region);
        }
    }
    all
}

/// is every kind in the region entered at `entry` one this binary supports inside a handler, and
/// can the region actually return control?
pub fn interrupt_region_is_supported(entry: &str, nodes: &[WorkflowNode]) -> bool {
    let Ok(region) = interrupt_region(entry, nodes) else {
        return false;
    };
    let by_id: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut saw_resume = false;
    for id in &region {
        let Some(node) = by_id.get(id.as_str()) else {
            return false;
        };
        if !graph_role(&node.kind).handler_safe {
            return false;
        }
        saw_resume |= node.kind == WorkflowNodeKind::Resume;
    }
    saw_resume
}

/// the declared interrupt handlers, ignoring any whose source this binary does not know.
///
/// an unknown source is not an error: a definition written against a newer binary must still load,
/// and the runtime simply never matches a source it cannot name. that is the fail-open rule.
///
/// metadata is the source of truth for the source-to-entry link and its enabled state. the graph
/// describes the linked region, while normalization wraps metadata-only legacy regions in an
/// explicit entry node.
pub fn interrupt_declarations(
    workflow: &WorkflowDefinition,
    _nodes: &[WorkflowNode],
) -> Vec<InterruptDeclaration> {
    workflow
        .definition
        .metadata
        .get("interrupts")
        .and_then(|value| value.decode::<Vec<InterruptDeclaration>>().ok())
        .unwrap_or_default()
}

/// [`interrupt_declarations`] for a caller that holds only the definition.
///
/// parsing still verifies that malformed or expanded graph data does not hide the declarations.
pub fn interrupt_declarations_for(workflow: &WorkflowDefinition) -> Vec<InterruptDeclaration> {
    parse_nodes(workflow)
        .map(|(_, nodes)| interrupt_declarations(workflow, &nodes))
        .unwrap_or_default()
}

/// an interrupt handler region must be a bounded side-channel: entered only by the interrupt,
/// exiting only by handing control back, and built from kinds that cannot park or fan out.
///
/// this is the `map` body rule with two extra clauses — the region is unreachable from `start`, and
/// every kind in it opted into [`GraphRole::handler_safe`]. the shared requirement is why it reuses
/// [`collect_body_region`] rather than growing a second reachability walk.
fn validate_interrupt_handlers(
    workflow: &WorkflowDefinition,
    start: &str,
    nodes: &[WorkflowNode],
) -> Result<(), WorkflowValidationError> {
    let declarations = interrupt_declarations(workflow, nodes);
    let linked_entries: HashSet<&str> = declarations
        .iter()
        .map(|declaration| declaration.handler.as_str())
        .collect();
    if let Some(unlinked) = nodes.iter().find(|node| {
        node.kind == WorkflowNodeKind::Interrupt && !linked_entries.contains(node.id.as_str())
    }) {
        return Err(WorkflowValidationError::InterruptHandlerNotIsolatable {
            handler: unlinked.id.as_str().to_string(),
            on: String::new(),
            reason: "interrupt entry is not linked by metadata".into(),
        });
    }
    if declarations.is_empty() {
        return Ok(());
    }
    let node_map: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    // an id no node can carry, so the walk from `start` stops only where the graph does.
    let main_flow = collect_body_region(start, "\0", &node_map)?;

    let mut claimed: HashMap<String, String> = HashMap::new();
    let mut sources_seen: HashSet<String> = HashSet::new();

    for declaration in &declarations {
        let handler = declaration.handler.as_str();
        let source = declaration.on.as_str();
        let not_isolatable =
            |reason: String| WorkflowValidationError::InterruptHandlerNotIsolatable {
                handler: handler.to_string(),
                on: source.to_string(),
                reason,
            };

        match declaration.source() {
            Some(runinator_models::interrupt::InterruptSource::Timer) => {
                if declaration
                    .interval_seconds
                    .is_none_or(|seconds| seconds <= 0)
                {
                    return Err(not_isolatable(
                        "timer interrupts require a positive `interval_seconds`".into(),
                    ));
                }
            }
            Some(_) if declaration.interval_seconds.is_some() => {
                return Err(not_isolatable(
                    "only timer interrupts may declare `interval_seconds`".into(),
                ));
            }
            _ => {}
        }

        // A timer declaration identifies itself by its handler entry, not just the shared source.
        // Other sources are singleton observations and retaining their one-handler rule keeps
        // their deterministic precedence intact.
        if declaration.source() != Some(runinator_models::interrupt::InterruptSource::Timer)
            && !sources_seen.insert(source.to_string())
        {
            return Err(not_isolatable(format!(
                "source '{source}' already has a handler; one handler per source"
            )));
        }
        let entry = node_map
            .get(handler)
            .ok_or_else(|| not_isolatable("handler node does not exist".into()))?;
        if !graph_role(&entry.kind).runnable_entry {
            return Err(not_isolatable(format!(
                "handler node is a {:?} node, which cannot be entered",
                entry.kind
            )));
        }
        if main_flow.contains(handler) {
            return Err(not_isolatable(
                "handler is reachable from the workflow start; a region must be entered only by \
                 its interrupt"
                    .into(),
            ));
        }

        let region = interrupt_region(handler, nodes)?;
        let mut has_resume = false;
        for region_id in &region {
            let region_node = node_map.get(region_id.as_str()).ok_or_else(|| {
                not_isolatable(format!("region node '{region_id}' does not exist"))
            })?;
            if region_node.kind == WorkflowNodeKind::Resume {
                has_resume = true;
            }
            if !graph_role(&region_node.kind).handler_safe {
                return Err(not_isolatable(format!(
                    "region node '{region_id}' is a {:?} node, which is not supported inside an \
                     interrupt handler",
                    region_node.kind
                )));
            }
            if main_flow.contains(region_id.as_str()) {
                return Err(not_isolatable(format!(
                    "region node '{region_id}' is also reachable from the workflow start"
                )));
            }
            if let Some(owner) = claimed.insert(region_id.clone(), handler.to_string())
                && owner != handler
            {
                return Err(not_isolatable(format!(
                    "region node '{region_id}' is already part of handler '{owner}'"
                )));
            }
        }
        if !has_resume {
            return Err(not_isolatable(
                "region never reaches a resume node, so it can never return control".into(),
            ));
        }

        // nothing outside the region may enter it or read its outputs.
        for other in nodes {
            let other_id = other.id.as_str();
            if region.contains(other_id) {
                continue;
            }
            for target in body_edges(other)? {
                if region.contains(target.as_str()) {
                    return Err(not_isolatable(format!(
                        "node '{other_id}' enters the region at '{}'",
                        target.as_str()
                    )));
                }
            }
            for reference in value_refs(other)? {
                if let WorkflowRefSource::NodeOutput(target) = reference.source
                    && region.contains(target.as_str())
                {
                    return Err(not_isolatable(format!(
                        "node '{other_id}' reads region output of '{}'",
                        target.as_str()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// all outgoing node references (transitions plus parameter-carried targets like switch cases and
/// nested control-flow branches).
fn body_edges(node: &WorkflowNode) -> Result<Vec<WorkflowNodeRef>, WorkflowValidationError> {
    let mut edges = transition_targets(&node.transitions);
    edges.extend(target_slots(node)?.into_iter().map(|slot| slot.target));
    Ok(edges)
}

/// the lock name a mutex node governs, defaulting to the node id (matching the reducer).
fn mutex_name(node: &WorkflowNode) -> String {
    node.parameters
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(node.id.as_str())
        .to_string()
}

/// true when a mutex node releases its lock (an end-of-section release node) rather than acquiring.
fn mutex_is_release(node: &WorkflowNode) -> bool {
    node.parameters
        .get("release")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// a mutex release node must be bracketed by its acquire: on every path from the start it must pass an
/// acquire for the same lock first. checked as reachability — from the start, following all edges but
/// treating each acquire for the lock as a barrier the walk stops at; if a release for that lock is
/// still reachable, the release can run before the lock is held. cycles are bounded by the visited
/// set (re-reaching an acquire just reinforces the existing hold at runtime, so it needs no error).
fn validate_mutex_sections(
    start: &str,
    nodes: &[WorkflowNode],
) -> Result<(), WorkflowValidationError> {
    let node_map: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let release_locks: HashSet<String> = nodes
        .iter()
        .filter(|node| node.kind == WorkflowNodeKind::Mutex && mutex_is_release(node))
        .map(mutex_name)
        .collect();

    for lock in release_locks {
        let mut visited = HashSet::new();
        let mut stack = vec![start.to_string()];
        while let Some(id) = stack.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let Some(node) = node_map.get(id.as_str()) else {
                continue;
            };
            if node.kind == WorkflowNodeKind::Mutex && mutex_name(node) == lock {
                if mutex_is_release(node) {
                    return Err(WorkflowValidationError::MutexReleaseBeforeAcquire {
                        node: id,
                        name: lock,
                    });
                }
                // an acquire for this lock is a barrier: paths beyond it hold the lock, so stop here.
                continue;
            }
            for edge in body_edges(node)? {
                stack.push(edge.as_str().to_string());
            }
        }
    }
    Ok(())
}

/// the set of nodes reachable from `target` without crossing `map_id`, following every outgoing edge.
fn collect_body_region(
    target: &str,
    map_id: &str,
    node_map: &HashMap<&str, &WorkflowNode>,
) -> Result<HashSet<String>, WorkflowValidationError> {
    let mut region = HashSet::new();
    let mut stack = vec![target.to_string()];
    while let Some(id) = stack.pop() {
        if id == map_id || !region.insert(id.clone()) {
            continue;
        }
        let Some(node) = node_map.get(id.as_str()) else {
            continue;
        };
        for edge in body_edges(node)? {
            let edge = edge.as_str();
            if edge != map_id && !region.contains(edge) {
                stack.push(edge.to_string());
            }
        }
    }
    Ok(region)
}

pub fn validate_workflow_with_providers(
    workflow: &WorkflowDefinition,
    providers: &[ProviderMetadata],
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    // config refs stay permissive (`any`) when no config schema is supplied.
    validate_workflow_with_config(workflow, providers, &RuninatorType::Any)
}

/// validate a workflow against provider metadata and a config schema; `config.*` references are
/// type-checked against `config_type` (an open `{ scope: { name: type } }` struct).
pub fn validate_workflow_with_config(
    workflow: &WorkflowDefinition,
    providers: &[ProviderMetadata],
    config_type: &RuninatorType,
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let (start, nodes) = validate_workflow(workflow)?;
    validate_workflow_types(workflow, &nodes, providers, config_type)?;
    Ok((start, nodes))
}

pub(crate) fn validate_graph_cycles(
    start: &str,
    nodes: &[WorkflowNode],
) -> Result<(), WorkflowValidationError> {
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    let node_map: HashMap<_, _> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    fn visit(
        id: &str,
        node_map: &HashMap<&str, &WorkflowNode>,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
    ) -> Result<(), WorkflowValidationError> {
        if stack.contains(id) {
            return Err(WorkflowValidationError::RefCycle(id.to_string()));
        }
        if visited.contains(id) {
            return Ok(());
        }

        visited.insert(id.to_string());
        stack.insert(id.to_string());

        if let Some(node) = node_map.get(id) {
            for target in transition_targets(&node.transitions) {
                // a back edge to a kind that is re-entered by design (or to a node with re-entry
                // switched on) is a loop, not a cycle error.
                if stack.contains(target.as_str())
                    && node_map.get(target.as_str()).is_some_and(|target_node| {
                        graph_role(&target_node.kind).reentrant || target_node.reentry.enabled
                    })
                {
                    continue;
                }
                visit(target.as_str(), node_map, visited, stack)?;
            }
        }

        stack.remove(id);
        Ok(())
    }

    visit(start, &node_map, &mut visited, &mut stack)
}

pub(crate) fn transition_targets(transitions: &WorkflowTransitions) -> Vec<WorkflowNodeRef> {
    let mut targets = Vec::new();
    for target in [
        &transitions.next,
        &transitions.on_success,
        &transitions.on_failure,
        &transitions.on_timeout,
        &transitions.on_reject,
    ]
    .into_iter()
    .flatten()
    {
        targets.push((*target).clone());
    }
    for branch in &transitions.branches {
        targets.push(branch.target.clone());
    }
    targets
}
