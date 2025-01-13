mod repository;

use chrono::{Duration, Local};
use log::{error, info};
use runinator_database::interfaces::DatabaseImpl;
use std::{
    collections::HashMap,
    sync::Arc,
    time,
};
use uuid::Uuid;

use runinator_config::Config;
use runinator_models::core::ScheduledTask;
use runinator_plugin::{load_libraries_from_path, plugin::Plugin, print_libs};
use tokio::sync::{Notify, Mutex};

async fn process_one_task(
    pool: &Arc<impl DatabaseImpl>,
    libraries: &HashMap<String, Plugin>,
    task: &ScheduledTask,
    task_handles: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    _config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Local::now().to_utc();
    if let Some(next_execution) = task.next_execution {
        if next_execution <= now {
            info!("Running action {}", &task.action_name);
            if let Some(plugin) = libraries.get(&task.action_name).cloned() {
                let timeout_duration: time::Duration =
                    Duration::seconds((task.timeout * 60) as i64)
                        .to_std()
                        .unwrap();
                let action_name = (&task.action_name).clone();
                let action_configuration = (&task.action_configuration).clone();
                let task_clone = task.clone();
                let pool_clone = pool.clone();
                let handle = tokio::spawn(async move {
                    process_plugin_task(action_name, action_configuration, task_clone, pool_clone, plugin).await.expect("plugin task failed");
                });

                let handle_index = {
                    let handle_uuid = Uuid::new_v4();
                    let mut handles = task_handles.lock().await;
                    handles.insert(handle_uuid.clone(), handle);
                    handle_uuid
                };

                let task_handles_clone = task_handles.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(timeout_duration).await;
                    let handles = task_handles_clone.lock().await;
                    if let Some(handle) = handles.get(&handle_index) {
                        handle.abort();
                    }
                });
            } else {
                error!("Action '{}' not found", task.action_name);
            }

            pool.update_task_next_execution(&task).await?;
        }
    }
    Ok(())
}

async fn process_plugin_task(action_name: String, action_configuration: Vec<u8>, task_clone: ScheduledTask, pool_clone: Arc<impl DatabaseImpl>, plugin: Plugin) 
-> Result<(), Box<dyn std::error::Error>> {
    let start_time = Local::now().to_utc();
    let start: time::Instant = time::Instant::now();
    plugin_call(&plugin, action_name, action_configuration);
    let duration_ms = start.elapsed().as_millis() as i64;
    pool_clone.log_task_run(&task_clone.name, start_time, duration_ms).await?;
    Ok(())
}

fn plugin_call(plugin: &Plugin, action_name: String, action_configuration: Vec<u8>) {
    let action_length = action_configuration.len();
    plugin.interface.lock().unwrap().call_service(action_name, action_configuration, action_length);
}

pub async fn scheduler_loop(
    pool: &Arc<impl DatabaseImpl>,
    notify: Arc<Notify>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    repository::initialize_database(pool.as_ref()).await?;
    let libraries = load_libraries_from_path(config.dll_path.as_str(), config.marker_function.as_str());
    print_libs(&libraries);
    let task_handles: Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    loop {
        let start: time::Instant = time::Instant::now();
        let notified = notify.notified();
        let map_clone = Arc::clone(&task_handles);
        tokio::select! {
            _ = notified => {
                info!("Scheduler received shutdown signal.");
                let mut handles = map_clone.lock().await;
                for (_, handle) in handles.drain() {
                    handle.abort();
                }
                break;
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(config.scheduler_frequency_seconds)) => {
                info!("Fetching tasks");
                let tasks = pool.fetch_all_tasks().await?;

                info!("Running tasks...");
                for task in tasks {
                    process_one_task(pool, &libraries, &task, task_handles.clone(), config).await?;
                }

                info!("Waiting for completed tasks!");
                let mut completed_tasks = vec![];
                let mut guard = task_handles.lock().await;
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
        let duration_s = start.elapsed().as_secs() as i64;
        info!("Scheduler took {} seconds to run", duration_s);
    }
    Ok(())
}
