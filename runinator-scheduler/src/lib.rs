mod repository;

use chrono::{Duration, Local, TimeDelta, Utc};
use cron::Schedule;
use log::{debug, error, info};
use repository::StaticProvider;
use runinator_database::interfaces::DatabaseImpl;
use std::{collections::HashMap, str::FromStr, sync::Arc, time};
use uuid::Uuid;

use runinator_config::Config;
use runinator_models::{core::ScheduledTask, errors::RuntimeError};
use runinator_plugin::{load_libraries_from_path, plugin::Plugin, print_libs, provider::Provider};
use tokio::sync::{Mutex, Notify};

type TaskHandleMap = Arc<Mutex<HashMap<Uuid, tokio::task::JoinHandle<()>>>>;

async fn process_one_task(
    pool: &Arc<impl DatabaseImpl>,
    libraries: &HashMap<String, Plugin>,
    task: &ScheduledTask,
    task_handles: TaskHandleMap,
    _config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Local::now().to_utc();

    if task.next_execution.is_none() || task.next_execution.unwrap() > now {
        set_initial_execution(pool, task).await?;
        return Ok(());
    }

    debug!("Running action {}", &task.action_name);

    let plugin = get_plugin_or_provider(libraries, task)?;

    let timeout_duration = Duration::seconds((task.timeout * 60) as i64)
        .to_std()
        .unwrap();
    let action_name = task.action_name.clone();
    let action_configuration = task.action_configuration.clone();
    let task_clone = task.clone();
    let pool_clone = pool.clone();

    let handle = tokio::spawn(async move {
        if let Err(e) = process_plugin_task(
            action_name,
            action_configuration,
            task_clone,
            pool_clone,
            plugin,
        )
        .await
        {
            // TODO: I don't want to panic here
            panic!("{}", e);
        }
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
        let mut handles = task_handles_clone.lock().await;
        if let Some(handle) = handles.remove(&handle_index) {
            handle.abort();
        }
    });

    set_next_execution_with_cron_statement(pool, task).await?;
    Ok(())
}

fn get_plugin_or_provider(
    libraries: &HashMap<String, Plugin>,
    task: &ScheduledTask,
) -> Result<StaticProvider, Box<dyn std::error::Error>> {
    if let Some(plugin) = libraries.get(&task.action_name) {
        return Ok(Box::new(plugin.clone()));
    }

    let providers = repository::get_providers();
    let match_provider: Option<StaticProvider> = providers
        .into_iter()
        .find(|provider| provider.name() == task.action_name)
        .map(|provider| provider);

    if let Some(provider) = match_provider {
        return Ok(provider);
    }

    Err(Box::new(RuntimeError::new(
        "2".to_string(),
        format!("Cannot find plugin/provider {}", task.action_name),
    )))
}

async fn set_next_execution_with_cron_statement(
    pool: &Arc<impl DatabaseImpl>,
    task: &ScheduledTask,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut task_next_execution = task.clone();
    let schedule_str = task_next_execution.cron_schedule.as_str();
    let schedule = Schedule::from_str(schedule_str)?;
    let next_upcoming = schedule.upcoming(Utc).take(1).next();
    let upcoming = match next_upcoming {
        Some(x) => x,
        None => Utc::now() + Duration::from(TimeDelta::seconds(60)),
    };
    task_next_execution.next_execution = Some(upcoming);
    pool.update_task_next_execution(&task_next_execution)
        .await?;
    Ok(())
}

async fn set_initial_execution(
    pool: &Arc<impl DatabaseImpl>,
    task: &ScheduledTask,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut task_clone = task.clone();
    task_clone.next_execution = Some(Utc::now());
    pool.update_task_next_execution(&task_clone).await?;
    Ok(())
}

async fn process_plugin_task(
    action_name: String,
    action_configuration: String,
    task_clone: ScheduledTask,
    pool_clone: Arc<impl DatabaseImpl>,
    plugin: Box<dyn Provider>
) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Local::now().to_utc();
    let start: time::Instant = time::Instant::now();
    plugin.call_service(action_name, action_configuration)?;
    let duration_ms = start.elapsed().as_millis() as i64;
    pool_clone
        .log_task_run(&task_clone.name, start_time, duration_ms)
        .await?;
    Ok(())
}

pub async fn scheduler_loop(
    pool: &Arc<impl DatabaseImpl>,
    notify: Arc<Notify>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    repository::initialize_database(pool.as_ref()).await?;
    let libraries = load_libraries_from_path(config.dll_path.as_str())?;
    print_libs(&libraries);
    let task_handles: TaskHandleMap = Arc::new(Mutex::new(HashMap::new()));
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
                debug!("Fetching tasks");
                let tasks = pool.fetch_all_tasks().await?;

                debug!("Running tasks...");
                for task in tasks {
                    process_one_task(pool, &libraries, &task, task_handles.clone(), config).await?;
                }

                debug!("Waiting for completed tasks!");
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
        let duration_s = start.elapsed().as_secs_f64();
        info!("Scheduler took {} seconds to run", duration_s);
    }
    Ok(())
}
