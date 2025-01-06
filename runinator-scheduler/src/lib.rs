use std::{collections::HashMap, sync::{Arc, Mutex}, time};

use chrono::{Duration, Local};
use runinator_models::models::ScheduledTask;

async fn process_one_task(pool: SqlitePool, libraries: HashMap<String, String>, task: &ScheduledTask) {
    let now = Local::now().naive_local();
    if let Some(next_execution) = task.next_execution {
        if next_execution <= now {
            if let Some(library_path) = libraries.get(&task.action_name) {
                let task_clone = task.clone();
                let pool_clone = pool.clone();
                let timeout_duration: time::Duration = Duration::seconds((task.timeout * 60) as i64).to_std().unwrap();
                let handle = tokio::spawn(async move {
                    let start_time = Local::now().naive_local();
                    let start: time::Instant = time::Instant::now();
                    let duration_ms =  start.elapsed().as_millis() as i64;
                    
                    let lib = Library::new(library_path).expect("Failed to load library");
                    unsafe {
                        let func: Symbol<unsafe extern "C" fn(&[u8])> =
                            lib.get(config.action_name_function.as_bytes()).expect("Failed to load function");
                        let start: time::Instant = time::Instant::now();
                        func(&task_clone.action_configuration);
                        start.elapsed().as_millis() as i64
                    };
                    log_task_run(&pool_clone, &task_clone.name, start_time, duration_ms).await;
                });
                task_handles.lock().unwrap().push(handle);

                tokio::spawn(async move {
                    tokio::time::sleep(timeout_duration).await;
                    handle.abort();
                });
            } else {
                error!("Library '{}' not found", task.action_name);
            }

            task.update_next_execution();

            update_task_next_execution(&pool, &task).await;
        }
    }
}

async fn scheduler_loop(pool: SqlitePool, libraries: HashMap<String, String>, notify: Arc<Notify>, config: &Config) {
    let task_handles = Arc::new(Mutex::new(Vec::new()));
    loop {
        let notified = notify.notified();
        tokio::select! {
            _ = notified => {
                info!("Scheduler received shutdown signal.");
                let handles = task_handles.lock().unwrap();
                for handle in handles.iter() {
                    handle.abort();
                }
                break;
            }
            _ = time::sleep(Duration::from_secs(1)) => {
                let tasks = fetch_all_tasks(&pool).await;
                for mut task in tasks {
                    process_one_task(&task);
                }
            }
        }
    }
}
