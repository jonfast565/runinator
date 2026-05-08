use std::collections::{HashMap, HashSet};

use runinator_models::workflows::{WorkflowDefinition, WorkflowMapping, WorkflowStep};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowValidationError {
    #[error("workflow definition.steps must be an array")]
    MissingSteps,
    #[error("workflow step '{0}' is duplicated")]
    DuplicateStep(String),
    #[error("workflow step '{step}' depends on missing step '{dependency}'")]
    MissingDependency { step: String, dependency: String },
    #[error("workflow contains a dependency cycle involving '{0}'")]
    Cycle(String),
    #[error("workflow step is invalid: {0}")]
    InvalidStep(String),
    #[error("workflow concurrency must be greater than zero")]
    InvalidConcurrency,
    #[error("workflow step '{step}' retry.max_attempts must be greater than zero")]
    InvalidRetry { step: String },
    #[error("workflow step '{step}' timeout_seconds must be greater than zero")]
    InvalidTimeout { step: String },
    #[error("workflow step '{step}' mapping references missing step '{from_step}'")]
    MissingMappingStep { step: String, from_step: String },
    #[error("workflow step '{step}' mapping pointer '{pointer}' is invalid")]
    InvalidJsonPointer { step: String, pointer: String },
}

pub fn parse_steps(
    workflow: &WorkflowDefinition,
) -> Result<Vec<WorkflowStep>, WorkflowValidationError> {
    let steps = workflow
        .definition
        .get("steps")
        .and_then(Value::as_array)
        .ok_or(WorkflowValidationError::MissingSteps)?;

    steps
        .iter()
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|err| WorkflowValidationError::InvalidStep(err.to_string()))
        })
        .collect()
}

pub fn validate_workflow(
    workflow: &WorkflowDefinition,
) -> Result<Vec<WorkflowStep>, WorkflowValidationError> {
    let steps = parse_steps(workflow)?;
    let mut seen = HashSet::new();
    for step in &steps {
        if !seen.insert(step.id.clone()) {
            return Err(WorkflowValidationError::DuplicateStep(step.id.clone()));
        }
    }

    let graph = steps
        .iter()
        .map(|step| (step.id.clone(), step.needs.clone()))
        .collect::<HashMap<_, _>>();

    for step in &steps {
        if step.retry.max_attempts <= 0 {
            return Err(WorkflowValidationError::InvalidRetry {
                step: step.id.clone(),
            });
        }
        if step
            .timeout_seconds
            .or(step.timeout)
            .is_some_and(|timeout| timeout <= 0)
        {
            return Err(WorkflowValidationError::InvalidTimeout {
                step: step.id.clone(),
            });
        }
        for dependency in &step.needs {
            if !graph.contains_key(dependency) {
                return Err(WorkflowValidationError::MissingDependency {
                    step: step.id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
        for mapping in &step.mappings {
            validate_mapping(step, mapping, &graph)?;
        }
    }

    if workflow
        .definition
        .get("concurrency")
        .and_then(Value::as_i64)
        .is_some_and(|concurrency| concurrency <= 0)
    {
        return Err(WorkflowValidationError::InvalidConcurrency);
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for step in graph.keys() {
        visit(step, &graph, &mut visiting, &mut visited)?;
    }

    Ok(steps)
}

pub fn workflow_concurrency(workflow: &WorkflowDefinition) -> usize {
    workflow
        .definition
        .get("concurrency")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(usize::MAX)
}

pub fn apply_mappings(
    base: &Value,
    step: &WorkflowStep,
    upstream_outputs: &HashMap<String, Value>,
) -> Result<Value, WorkflowValidationError> {
    let mut result = base.clone();
    for mapping in &step.mappings {
        let Some(source) = upstream_outputs.get(&mapping.from_step) else {
            return Err(WorkflowValidationError::MissingMappingStep {
                step: step.id.clone(),
                from_step: mapping.from_step.clone(),
            });
        };
        let Some(value) = source.pointer(&mapping.from_pointer) else {
            continue;
        };
        set_json_pointer(&mut result, &mapping.to_pointer, value.clone()).map_err(|_| {
            WorkflowValidationError::InvalidJsonPointer {
                step: step.id.clone(),
                pointer: mapping.to_pointer.clone(),
            }
        })?;
    }
    Ok(result)
}

fn validate_mapping(
    step: &WorkflowStep,
    mapping: &WorkflowMapping,
    graph: &HashMap<String, Vec<String>>,
) -> Result<(), WorkflowValidationError> {
    if !graph.contains_key(&mapping.from_step) {
        return Err(WorkflowValidationError::MissingMappingStep {
            step: step.id.clone(),
            from_step: mapping.from_step.clone(),
        });
    }
    for pointer in [&mapping.from_pointer, &mapping.to_pointer] {
        if !is_valid_pointer(pointer) {
            return Err(WorkflowValidationError::InvalidJsonPointer {
                step: step.id.clone(),
                pointer: pointer.clone(),
            });
        }
    }
    Ok(())
}

fn is_valid_pointer(pointer: &str) -> bool {
    pointer.is_empty() || pointer.starts_with('/')
}

fn set_json_pointer(target: &mut Value, pointer: &str, value: Value) -> Result<(), ()> {
    if pointer.is_empty() {
        *target = value;
        return Ok(());
    }
    let parts = pointer
        .strip_prefix('/')
        .ok_or(())?
        .split('/')
        .map(|part| part.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    let mut current = target;
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !current.is_object() {
            *current = Value::Object(Default::default());
        }
        current = current
            .as_object_mut()
            .ok_or(())?
            .entry(part.clone())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    let Some(last) = parts.last() else {
        return Err(());
    };
    if !current.is_object() {
        *current = Value::Object(Default::default());
    }
    current
        .as_object_mut()
        .ok_or(())?
        .insert(last.clone(), value);
    Ok(())
}

fn visit(
    step: &str,
    graph: &HashMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> Result<(), WorkflowValidationError> {
    if visited.contains(step) {
        return Ok(());
    }
    if !visiting.insert(step.to_string()) {
        return Err(WorkflowValidationError::Cycle(step.to_string()));
    }

    if let Some(dependencies) = graph.get(step) {
        for dependency in dependencies {
            visit(dependency, graph, visiting, visited)?;
        }
    }

    visiting.remove(step);
    visited.insert(step.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(definition: Value) -> WorkflowDefinition {
        WorkflowDefinition {
            id: Some(1),
            name: "test".into(),
            version: 1,
            enabled: true,
            input_schema: Value::Null,
            definition,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn validates_acyclic_workflow() {
        let wf = workflow(serde_json::json!({
            "steps": [
                { "id": "build", "task_id": 1 },
                { "id": "test", "task_id": 2, "needs": ["build"] }
            ]
        }));

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn rejects_cycles() {
        let wf = workflow(serde_json::json!({
            "steps": [
                { "id": "a", "task_id": 1, "needs": ["b"] },
                { "id": "b", "task_id": 2, "needs": ["a"] }
            ]
        }));

        assert!(matches!(
            validate_workflow(&wf),
            Err(WorkflowValidationError::Cycle(_))
        ));
    }

    #[test]
    fn rejects_bad_mapping_pointer() {
        let wf = workflow(serde_json::json!({
            "steps": [
                { "id": "a", "task_id": 1 },
                {
                    "id": "b",
                    "task_id": 2,
                    "needs": ["a"],
                    "mappings": [{ "from_step": "a", "from_pointer": "bad", "to_pointer": "/value" }]
                }
            ]
        }));

        assert!(matches!(
            validate_workflow(&wf),
            Err(WorkflowValidationError::InvalidJsonPointer { .. })
        ));
    }

    #[test]
    fn rejects_missing_dependencies() {
        let wf = workflow(serde_json::json!({
            "steps": [
                { "id": "test", "task_id": 2, "needs": ["build"] }
            ]
        }));

        assert!(matches!(
            validate_workflow(&wf),
            Err(WorkflowValidationError::MissingDependency { .. })
        ));
    }

    #[test]
    fn rejects_invalid_retry_timeout_and_concurrency() {
        let bad_retry = workflow(serde_json::json!({
            "steps": [{ "id": "a", "task_id": 1, "retry": { "max_attempts": 0 } }]
        }));
        assert!(matches!(
            validate_workflow(&bad_retry),
            Err(WorkflowValidationError::InvalidRetry { .. })
        ));

        let bad_timeout = workflow(serde_json::json!({
            "steps": [{ "id": "a", "task_id": 1, "timeout_seconds": 0 }]
        }));
        assert!(matches!(
            validate_workflow(&bad_timeout),
            Err(WorkflowValidationError::InvalidTimeout { .. })
        ));

        let bad_concurrency = workflow(serde_json::json!({
            "concurrency": 0,
            "steps": [{ "id": "a", "task_id": 1 }]
        }));
        assert!(matches!(
            validate_workflow(&bad_concurrency),
            Err(WorkflowValidationError::InvalidConcurrency)
        ));
    }

    #[test]
    fn maps_output_into_parameters() {
        let step: WorkflowStep = serde_json::from_value(serde_json::json!({
            "id": "b",
            "task_id": 2,
            "mappings": [{ "from_step": "a", "from_pointer": "/sha", "to_pointer": "/build/sha" }]
        }))
        .unwrap();
        let upstream = HashMap::from([("a".into(), serde_json::json!({ "sha": "abc123" }))]);

        let mapped = apply_mappings(&serde_json::json!({}), &step, &upstream).unwrap();
        assert_eq!(
            mapped.pointer("/build/sha").and_then(Value::as_str),
            Some("abc123")
        );
    }
}
