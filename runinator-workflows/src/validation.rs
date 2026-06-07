use std::collections::{HashMap, HashSet};

use runinator_models::{
    providers::ProviderMetadata,
    types::RuninatorType,
    workflows::{
        WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowTransitions,
    },
};

use crate::conditions::validate_condition;
use crate::errors::WorkflowValidationError;
use crate::parameters::{
    parameter_targets, parse_map_parameters, validate_control_node_parameters, value_refs,
};
use crate::refs::expand_workflow_refs;
use crate::types::WorkflowRefSource;
use crate::typing::validate_workflow_types;

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

    for node in &nodes {
        if node.kind == WorkflowNodeKind::Action && node.action.is_none() {
            return Err(WorkflowValidationError::MissingAction(
                node.id.as_str().to_string(),
            ));
        }
        if node.kind == WorkflowNodeKind::Subflow
            && node.subflow_id.is_none()
            && node
                .subflow
                .workflow_name
                .as_ref()
                .is_none_or(|name| name.trim().is_empty())
        {
            return Err(WorkflowValidationError::MissingSubflowTarget(
                node.id.as_str().to_string(),
            ));
        }
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
        validate_condition(&node.condition)?;
        validate_control_node_parameters(node)?;
        for target in transition_targets(&node.transitions) {
            if !ids.contains(target.as_str()) {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.as_str().to_string(),
                    target: target.into_string(),
                });
            }
        }
        for target in parameter_targets(node)? {
            if !ids.contains(target.as_str()) {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.as_str().to_string(),
                    target: target.into_string(),
                });
            }
        }
        for reference in value_refs(node)? {
            if let WorkflowRefSource::NodeOutput(target) = reference.source
                && !ids.contains(target.as_str())
            {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.as_str().to_string(),
                    target: target.into_string(),
                });
            }
        }
        if let Some(target) = node.reentry.on_exhausted.as_ref()
            && !ids.contains(target.as_str())
        {
            return Err(WorkflowValidationError::MissingTransition {
                node: node.id.as_str().to_string(),
                target: target.clone().into_string(),
            });
        }
    }

    validate_graph_cycles(&start, &nodes)?;
    validate_map_concurrency_bodies(&nodes)?;

    Ok((start, nodes))
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
            if matches!(
                region_node.kind,
                WorkflowNodeKind::Start | WorkflowNodeKind::End | WorkflowNodeKind::Fail
            ) {
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

/// all outgoing node references (transitions plus parameter-carried targets like switch cases and
/// nested control-flow branches).
fn body_edges(node: &WorkflowNode) -> Result<Vec<WorkflowNodeRef>, WorkflowValidationError> {
    let mut edges = transition_targets(&node.transitions);
    edges.extend(parameter_targets(node)?);
    Ok(edges)
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
                if stack.contains(target.as_str())
                    && node_map.get(target.as_str()).is_some_and(|target_node| {
                        matches!(
                            target_node.kind,
                            WorkflowNodeKind::Loop
                                | WorkflowNodeKind::Try
                                | WorkflowNodeKind::Map
                                | WorkflowNodeKind::Race
                        ) || target_node.reentry.enabled
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
