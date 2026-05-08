use crate::models::TaskRunRequest;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunRequest, RunStatus, RunSummary},
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStepRun},
};
use serde_json::Value;

pub async fn add_task<T: DatabaseImpl>(
    db: &T,
    scheduled_task: &ScheduledTask,
) -> Result<TaskResponse, SendableError> {
    db.upsert_task(scheduled_task).await?;
    Ok(TaskResponse {
        success: true,
        message: "Task added successfully".to_string(),
    })
}

pub async fn update_task<T: DatabaseImpl>(
    db: &T,
    scheduled_task: &ScheduledTask,
) -> Result<TaskResponse, SendableError> {
    db.upsert_task(scheduled_task).await?;
    Ok(TaskResponse {
        success: true,
        message: "Task updated successfully".to_string(),
    })
}

pub async fn delete_task<T: DatabaseImpl>(
    db: &T,
    task_id: i64,
) -> Result<TaskResponse, SendableError> {
    db.delete_task(task_id).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Task with ID {} deleted successfully", task_id),
    })
}

pub async fn request_run<T: DatabaseImpl>(
    db: &T,
    task_id: i64,
) -> Result<TaskResponse, SendableError> {
    db.request_immediate_run(task_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Run requested".into(),
    })
}

pub async fn fetch_tasks<T: DatabaseImpl>(db: &T) -> Result<Vec<ScheduledTask>, SendableError> {
    let result = db.fetch_all_tasks().await?;
    Ok(result)
}

pub async fn fetch_task_runs<T: DatabaseImpl>(
    db: &T,
    start: i64,
    end: i64,
) -> Result<Vec<TaskRun>, SendableError> {
    let result = db.fetch_task_runs(start, end).await?;
    Ok(result)
}

pub async fn log_task_run<T: DatabaseImpl>(
    db: &T,
    input: &TaskRunRequest,
) -> Result<TaskResponse, SendableError> {
    db.log_task_run(input.task_id, input.started_at, input.duration_ms)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Task run recorded".into(),
    })
}

fn merge_json_object(defaults: &Value, parameters: &Value) -> Value {
    match (defaults, parameters) {
        (Value::Object(defaults), Value::Object(parameters)) => {
            let mut merged = defaults.clone();
            for (key, value) in parameters {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        (_, Value::Null) => defaults.clone(),
        _ => parameters.clone(),
    }
}

pub async fn create_run<T: DatabaseImpl>(
    db: &T,
    task_id: i64,
    request: &RunRequest,
) -> Result<RunSummary, SendableError> {
    let task = db.fetch_task_by_id(task_id).await?.ok_or_else(|| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Task {task_id} not found"),
        )) as SendableError
    })?;
    let parameters = merge_json_object(&task.default_parameters, &request.parameters);
    validate_json_schema(&task.input_schema, &parameters).map_err(|message| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            message,
        )) as SendableError
    })?;
    db.create_task_run(task_id, parameters, request.trigger.clone(), None, None)
        .await
}

fn validate_json_schema(schema: &Value, value: &Value) -> Result<(), String> {
    let Some(schema_object) = schema.as_object() else {
        return Ok(());
    };
    if let Some(schema_type) = schema_object.get("type").and_then(Value::as_str) {
        validate_type(schema_type, value, "")?;
    }
    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        let Some(object) = value.as_object() else {
            return Err("parameters must be an object to validate required fields".into());
        };
        for field in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(field) {
                return Err(format!("missing required parameter '{field}'"));
            }
        }
    }
    if let Some(properties) = schema_object.get("properties").and_then(Value::as_object) {
        let Some(object) = value.as_object() else {
            return Ok(());
        };
        for (field, property_schema) in properties {
            let Some(field_value) = object.get(field) else {
                continue;
            };
            if let Some(field_type) = property_schema.get("type").and_then(Value::as_str) {
                validate_type(field_type, field_value, field)?;
            }
        }
    }
    Ok(())
}

fn validate_type(expected: &str, value: &Value, field: &str) -> Result<(), String> {
    let valid = match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => true,
    };
    if valid {
        Ok(())
    } else if field.is_empty() {
        Err(format!("parameters must be JSON type '{expected}'"))
    } else {
        Err(format!(
            "parameter '{field}' must be JSON type '{expected}'"
        ))
    }
}

pub async fn fetch_run<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
) -> Result<Option<RunSummary>, SendableError> {
    db.fetch_run(run_id).await
}

pub async fn fetch_runs_for_task<T: DatabaseImpl>(
    db: &T,
    task_id: i64,
) -> Result<Vec<RunSummary>, SendableError> {
    db.fetch_runs_for_task(task_id).await
}

pub async fn fetch_runs_by_status<T: DatabaseImpl>(
    db: &T,
    status: RunStatus,
) -> Result<Vec<RunSummary>, SendableError> {
    db.fetch_runs_by_status(status).await
}

pub async fn update_run_status<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    status: RunStatus,
    output_json: Option<Value>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_run_status(run_id, status, output_json, message)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Run updated".into(),
    })
}

pub async fn fetch_run_chunks<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    cursor: Option<i64>,
    limit: i64,
) -> Result<Vec<RunChunk>, SendableError> {
    db.fetch_run_chunks(run_id, cursor, limit).await
}

pub async fn append_run_chunk<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    chunk: &NewRunChunk,
) -> Result<TaskResponse, SendableError> {
    db.append_run_chunk(run_id, chunk).await?;
    Ok(TaskResponse {
        success: true,
        message: "Run chunk appended".into(),
    })
}

pub async fn fetch_run_artifacts<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
) -> Result<Vec<RunArtifact>, SendableError> {
    db.fetch_run_artifacts(run_id).await
}

pub async fn add_run_artifact<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    artifact: &NewRunArtifact,
) -> Result<TaskResponse, SendableError> {
    db.add_run_artifact(run_id, artifact).await?;
    Ok(TaskResponse {
        success: true,
        message: "Run artifact recorded".into(),
    })
}

pub async fn fetch_artifact<T: DatabaseImpl>(
    db: &T,
    artifact_id: i64,
) -> Result<Option<RunArtifact>, SendableError> {
    db.fetch_artifact(artifact_id).await
}

pub async fn upsert_workflow<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    runinator_workflows::validate_workflow(workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    db.upsert_workflow(workflow).await
}

pub async fn fetch_workflows<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    db.fetch_workflows().await
}

pub async fn fetch_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<Option<WorkflowDefinition>, SendableError> {
    db.fetch_workflow(workflow_id).await
}

pub async fn delete_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow(workflow_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow deleted".into(),
    })
}

pub async fn create_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
    parameters: Value,
) -> Result<WorkflowRun, SendableError> {
    db.create_workflow_run(workflow_id, parameters).await
}

pub async fn fetch_workflow_runs_by_status<T: DatabaseImpl>(
    db: &T,
    status: RunStatus,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_by_status(status).await
}

pub async fn fetch_workflow_runs_for_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_for_workflow(workflow_id).await
}

pub async fn update_workflow_run_status<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    status: RunStatus,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_run_status(workflow_run_id, status, message)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run updated".into(),
    })
}

pub async fn fetch_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<Option<(WorkflowRun, Vec<WorkflowStepRun>)>, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(None);
    };
    let steps = db.fetch_workflow_step_runs(workflow_run_id).await?;
    Ok(Some((run, steps)))
}

pub async fn create_workflow_step_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    step_id: String,
    parameters: Value,
) -> Result<WorkflowStepRun, SendableError> {
    db.create_workflow_step_run(workflow_run_id, step_id, parameters)
        .await
}

pub async fn update_workflow_step_run<T: DatabaseImpl>(
    db: &T,
    step_run_id: i64,
    status: RunStatus,
    task_run_id: Option<i64>,
    attempt: Option<i64>,
    parameters: Option<Value>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_step_run(
        step_run_id,
        status,
        task_run_id,
        attempt,
        parameters,
        message,
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow step run updated".into(),
    })
}
