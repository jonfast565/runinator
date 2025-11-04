use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::Utc;
use log::{error, info, warn};
use runinator_comm::TaskResult;
use runinator_models::core::ScheduledTask;
use runinator_plugin::plugin::Plugin;
use tokio::time;
use uuid::Uuid;

use crate::provider_repository::resolve_provider;

pub async fn execute_task(
    libraries: Arc<HashMap<String, Plugin>>,
    command_id: Uuid,
    task: ScheduledTask,
) -> TaskResult {
    let started_at = Utc::now();
    let timeout = task.timeout.max(1) as u64;
    let action_name = task.action_name.clone();
    let action_configuration = task.action_configuration.clone();

    match resolve_provider(&libraries, &task) {
        Ok(provider) => {
            let handle = tokio::task::spawn_blocking(move || {
                provider.call_service(action_name, action_configuration, task.timeout)
            });

            match time::timeout(Duration::from_secs(timeout), handle).await {
                Ok(join_result) => match join_result {
                    Ok(Ok(_)) => {
                        let finished_at = Utc::now();
                        info!(
                            "Task {} completed successfully",
                            task.id.unwrap_or_default()
                        );
                        TaskResult {
                            command_id,
                            success: true,
                            started_at,
                            finished_at,
                            message: None,
                        }
                    }
                    Ok(Err(err)) => {
                        error!(
                            "Provider execution error for task {}: {}",
                            task.id.unwrap_or_default(),
                            err
                        );
                        TaskResult {
                            command_id,
                            success: false,
                            started_at,
                            finished_at: Utc::now(),
                            message: Some(err.to_string()),
                        }
                    }
                    Err(err) => {
                        error!("Task panicked: {:?}", err);
                        TaskResult {
                            command_id,
                            success: false,
                            started_at,
                            finished_at: Utc::now(),
                            message: Some("Task panicked during execution".into()),
                        }
                    }
                },
                Err(_) => {
                    warn!(
                        "Task {} exceeded timeout of {} seconds",
                        task.id.unwrap_or_default(),
                        timeout
                    );
                    TaskResult {
                        command_id,
                        success: false,
                        started_at,
                        finished_at: Utc::now(),
                        message: Some(format!("Task timed out after {} seconds", timeout)),
                    }
                }
            }
        }
        Err(err) => {
            error!(
                "Failed to resolve provider for task {}: {}",
                task.id.unwrap_or_default(),
                err
            );
            TaskResult {
                command_id,
                success: false,
                started_at,
                finished_at: Utc::now(),
                message: Some(err.to_string()),
            }
        }
    }
}
