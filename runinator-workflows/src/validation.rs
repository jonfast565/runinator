use std::collections::{HashMap, HashSet};

use runinator_models::value::Value;
use runinator_models::{
    providers::ProviderMetadata,
    workflows::{
        WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowTransitions,
    },
};

use crate::conditions::validate_condition;
use crate::errors::WorkflowValidationError;
use crate::parameters::{parameter_targets, validate_control_node_parameters, value_refs};
use crate::refs::expand_workflow_refs;
use crate::types::WorkflowRefSource;
use crate::typing::validate_workflow_types;

pub fn parse_nodes(
    workflow: &WorkflowDefinition,
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let definition = expand_workflow_refs(workflow)?;
    let start = definition
        .get("start")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(WorkflowValidationError::MissingStart)?
        .to_string();
    let nodes = definition
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(WorkflowValidationError::MissingNodes)?;
    let nodes = nodes
        .iter()
        .map(|value| {
            serde_json::from_value(value.clone().into())
                .map_err(|err| WorkflowValidationError::InvalidNode(err.to_string()))
        })
        .collect::<Result<Vec<WorkflowNode>, _>>()?;
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

    Ok((start, nodes))
}

pub fn validate_workflow_with_providers(
    workflow: &WorkflowDefinition,
    providers: &[ProviderMetadata],
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let (start, nodes) = validate_workflow(workflow)?;
    validate_workflow_types(workflow, &nodes, providers)?;
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
