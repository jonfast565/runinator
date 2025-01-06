use chrono::{Duration, Local};
use log::{error, info};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time,
};
use uuid::Uuid;

use runinator_config::Config;
use runinator_database::{fetch_all_tasks, log_task_run, update_task_next_execution};
use runinator_models::models::ScheduledTask;
use sqlx::SqlitePool;
use tokio::sync::Notify;

async fn process_one_task(
    pool: &SqlitePool,
    libraries: &HashMap<String, String>,
    task: &ScheduledTask,
    task_handles: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    config: &Config,
) {
    let now = Local::now().to_utc();
    if let Some(next_execution) = task.next_execution {
        if next_execution <= now {
            if let Some(library_path) = libraries.get(&task.action_name) {
                let task_clone = task.clone();
                let pool_clone = pool.clone();
                let timeout_duration: time::Duration =
                    Duration::seconds((task.timeout * 60) as i64)
                        .to_std()
                        .unwrap();
                let handle = tokio::spawn(async move {
                    let start_time = Local::now().to_utc();
                    let start: time::Instant = time::Instant::now();
                    // do something here

                    let duration_ms = start.elapsed().as_millis() as i64;
                    log_task_run(&pool_clone, &task_clone.name, start_time, duration_ms).await;
                });

                let handle_index = {
                    let handle_uuid = Uuid::new_v4();
                    let mut handles = task_handles.lock().unwrap();
                    handles.insert(handle_uuid.clone(), handle);
                    handle_uuid
                };

                let task_handles_clone = task_handles.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(timeout_duration).await;
                    let handles = task_handles_clone.lock().unwrap();
                    if let Some(handle) = handles.get(&handle_index) {
                        handle.abort();
                    }
                });
            } else {
                error!("Library '{}' not found", task.action_name);
            }

            update_task_next_execution(&pool, &task).await;
        }
    }
}

pub async fn scheduler_loop(
    pool: &SqlitePool,
    libraries: &HashMap<String, String>,
    notify: Arc<Notify>,
    config: &Config,
) {
    let task_handles: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    loop {
        let notified = notify.notified();
        let map_clone = Arc::clone(&task_handles);
        tokio::select! {
            _ = notified => {
                info!("Scheduler received shutdown signal.");
                let mut handles = map_clone.lock().unwrap();
                for (_, handle) in handles.drain() {
                    handle.abort();
                }
                break;
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                info!("Fetching tasks");
                let tasks = fetch_all_tasks(pool).await;

                info!("Running tasks...");
                for task in tasks {
                    process_one_task(pool, &libraries, &task, task_handles.clone(), config).await;
                }

                info!("Waiting for completed tasks!");
                let mut completed_tasks = vec![];
                let mut guard = task_handles.lock().unwrap();
                for (uuid, handle) in guard.drain() {
                    match handle.await {
                        Ok(_) => {
                            completed_tasks.push(uuid);
                        }
                        Err(e) => {
                            error!("Task {:?} failed: {:?}", uuid, e);
                        }
                    }
                }

                for uuid in completed_tasks {
                    guard.remove(&uuid);
                }
            }
        }
    }
}
